use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Meter {
    Electricity,
    Export,
    Gas,
    Solar,
    Water,
}

impl Meter {
    pub const ALL: [Self; 5] = [
        Self::Electricity,
        Self::Export,
        Self::Gas,
        Self::Solar,
        Self::Water,
    ];
    pub fn title(self) -> &'static str {
        match self {
            Self::Electricity => "Electricity",
            Self::Export => "Grid export",
            Self::Gas => "Gas",
            Self::Solar => "Solar",
            Self::Water => "Water",
        }
    }
    pub fn unit(self, period: Period) -> &'static str {
        match (self, period.flow()) {
            (Self::Electricity | Self::Export | Self::Solar, true) => "W",
            (Self::Gas, true) => "L/h",
            (Self::Water, true) => "L/min",
            (Self::Electricity | Self::Export | Self::Solar, false) => "kWh",
            (Self::Gas | Self::Water, false) => "m³",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Period {
    Hour,
    Day,
    Week,
    Month,
    Year,
}

impl Period {
    pub const ALL: [Self; 5] = [Self::Hour, Self::Day, Self::Week, Self::Month, Self::Year];
    pub fn title(self) -> &'static str {
        match self {
            Self::Hour => "Hour",
            Self::Day => "Day",
            Self::Week => "Week",
            Self::Month => "Month",
            Self::Year => "Year",
        }
    }
    pub fn flow(self) -> bool {
        self == Self::Hour
    }
    fn query(self) -> (&'static str, u64, u64, usize) {
        match self {
            Self::Hour => ("5min", 3600, 300, 12),
            Self::Day => ("5yrhours", 86400, 3600, 24),
            Self::Week => ("5yrhours", 7 * 86400, 3600, 28),
            Self::Month => ("10yrdays", 31 * 86400, 86400, 31),
            Self::Year => ("10yrdays", 365 * 86400, 86400, 52),
        }
    }
}

#[derive(serde::Serialize)]
pub struct Chart {
    pub bars: Vec<Option<f64>>,
    pub unit: &'static str,
    pub total: Option<f64>,
    pub partial: bool,
    pub from: u64,
    pub to: u64,
}

type Samples = BTreeMap<u64, Option<f64>>;

fn samples(logger: &str, archive: &str, from: u64, to: u64, count: u64) -> Result<Samples, String> {
    let query = format!("action=getRrdData&loggerName={logger}&rra={archive}&from={from}&to={to}&nullForNaN=1&samples={count}");
    let response = crate::thermostat::firmware_get("hcb_rrd", &query)?;
    let json: Value =
        serde_json::from_str(&response).map_err(|_| "Invalid meter archive response")?;
    if let Some(message) = json["error"].as_str() {
        return match message {
            "unknown logger" | "unknown rra" => Ok(Samples::new()),
            _ => Err(message.to_owned()),
        };
    }
    let rows = json.as_object().ok_or("Invalid meter archive shape")?;
    Ok(rows
        .iter()
        .filter_map(|(time, value)| {
            Some((
                time.parse::<u64>().ok()?,
                value
                    .as_f64()
                    .filter(|value| value.is_finite() && value.abs() <= 1e10),
            ))
        })
        .collect())
}

fn changes(readings: Samples, interval: u64) -> Samples {
    let mut previous = None;
    let mut result = Samples::new();
    for (time, value) in readings {
        if let Some((before_time, before_value)) = previous {
            let change = match (before_value, value) {
                (Some(before), Some(after))
                    if time - before_time == interval && after >= before =>
                {
                    Some((after - before) / 1000.0)
                }
                _ => None,
            };
            result.insert(time, change);
        }
        previous = Some((time, value));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_deltas_distinguish_zero_usage_gaps_and_counter_resets() {
        let readings = [
            (0, Some(5000.0)),
            (60, Some(5000.0)),
            (120, Some(6200.0)),
            (240, Some(6600.0)),
            (300, Some(20.0)),
            (360, None),
            (420, Some(500.0)),
        ]
        .into();
        assert_eq!(
            changes(readings, 60),
            [
                (60, Some(0.0)),
                (120, Some(1.2)),
                (240, None),
                (300, None),
                (360, None),
                (420, None)
            ]
            .into()
        );
    }
}

pub fn load(meter: Meter, period: Period) -> Result<Chart, String> {
    let (mut archive, window, mut interval, buckets) = period.query();
    if meter == Meter::Water && matches!(period, Period::Day | Period::Week) {
        archive = "10yrhours";
    }
    if meter == Meter::Export && matches!(period, Period::Day | Period::Week) {
        archive = "5minweek";
        interval = 300;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let end = now - now % interval;
    let start = end.saturating_sub(window);
    let count = window / interval + 2;
    let (primary, extra) = match (meter, period.flow()) {
        (Meter::Electricity, true) => ("elec_flow", None),
        (Meter::Export, true) => ("elec_produ_flow", None),
        (Meter::Gas, true) => ("gas_flow", None),
        (Meter::Solar, true) => ("elec_solar_flow", None),
        (Meter::Water, true) => ("water_flow", None),
        (Meter::Electricity, false) => ("elec_quantity_nt", Some("elec_quantity_lt")),
        (Meter::Export, false) if matches!(period, Period::Day | Period::Week) => {
            ("elec_received_", None)
        }
        (Meter::Export, false) => ("elec_quantity_nt_produ", Some("elec_quantity_lt_produ")),
        (Meter::Gas, false) => ("gas_quantity", None),
        (Meter::Solar, false) => ("elec_solar_quantity", None),
        (Meter::Water, false) => ("water_quantity", None),
    };
    let mut readings = samples(primary, archive, start, end, count)?;
    let mut partial = false;
    if !period.flow() {
        readings = changes(readings, interval);
        if let Some(extra) = extra {
            let secondary = changes(samples(extra, archive, start, end, count)?, interval);
            if secondary.is_empty() || readings.is_empty() {
                partial = true;
                if readings.is_empty() {
                    readings = secondary;
                }
            } else {
                for (&time, value) in &mut readings {
                    *value = value
                        .zip(secondary.get(&time).copied().flatten())
                        .map(|(a, b)| a + b);
                }
            }
        }
    }
    let mut sums = vec![(0.0, 0); buckets];
    let mut observed = 0;
    for (time, reading) in readings {
        if time <= start || time > end {
            continue;
        }
        let Some(reading) = reading else {
            continue;
        };
        let index = (((time - start - 1) * buckets as u64) / window) as usize;
        sums[index].0 += reading;
        sums[index].1 += 1;
        observed += 1;
    }
    let sum: f64 = sums.iter().map(|(sum, _)| *sum).sum();
    let total = (!period.flow() && observed > 0).then_some(sum);
    let bars = sums
        .into_iter()
        .map(|(sum, count)| {
            (count > 0).then(|| {
                if period.flow() {
                    sum / f64::from(count)
                } else {
                    sum
                }
            })
        })
        .collect();
    Ok(Chart {
        bars,
        unit: meter.unit(period),
        total,
        partial: partial || observed < window / interval,
        from: start,
        to: end,
    })
}
