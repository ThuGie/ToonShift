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
    pub partial_bars: Vec<bool>,
    pub unit: &'static str,
    pub total: Option<f64>,
    pub partial: bool,
    pub from: u64,
    pub to: u64,
    #[serde(skip)]
    tariff_usage: [Option<f64>; 2],
}

#[derive(Clone, Copy, Debug, Default, serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Rates {
    pub standard: Option<u32>,
    pub off_peak: Option<u32>,
    pub gas: Option<u32>,
}

#[derive(Clone, Copy)]
pub enum Rate {
    Standard,
    OffPeak,
    Gas,
}

impl Rate {
    pub const ALL: [Self; 3] = [Self::Standard, Self::OffPeak, Self::Gas];

    pub fn title(self) -> &'static str {
        match self {
            Self::Standard => "Standard electricity (€/kWh)",
            Self::OffPeak => "Off-peak electricity (€/kWh)",
            Self::Gas => "Gas rate (€/m³)",
        }
    }

    fn field(self, rates: &mut Rates) -> &mut Option<u32> {
        match self {
            Self::Standard => &mut rates.standard,
            Self::OffPeak => &mut rates.off_peak,
            Self::Gas => &mut rates.gas,
        }
    }

    pub fn value(self, mut rates: Rates) -> Option<u32> {
        *self.field(&mut rates)
    }

    pub fn save(self, rates: &mut Rates, input: &str) -> Result<(), &'static str> {
        let value = if input.is_empty() {
            None
        } else {
            let (whole, fraction) = input.split_once(['.', ',']).unwrap_or((input, ""));
            if whole.is_empty()
                || fraction.len() > 3
                || !whole
                    .bytes()
                    .chain(fraction.bytes())
                    .all(|c| c.is_ascii_digit())
            {
                return Err("Enter a rate from 0 to 99.999.");
            }
            let whole: u32 = whole
                .parse()
                .map_err(|_| "Enter a rate from 0 to 99.999.")?;
            if whole > 99 {
                return Err("Enter a rate from 0 to 99.999.");
            }
            let fraction =
                fraction.parse::<u32>().unwrap_or(0) * 10_u32.pow(3 - fraction.len() as u32);
            Some(whole * 1000 + fraction)
        };
        *self.field(rates) = value;
        Ok(())
    }
}

impl Chart {
    pub fn csv(&self) -> String {
        use std::fmt::Write as _;
        let mut csv =
            String::from("from_unix,to_unix,value,unit,window_partial,bucket_partial\r\n");
        let count = self.bars.len() as u64;
        let window = self.to - self.from;
        for (index, value) in self.bars.iter().enumerate() {
            let index = index as u64;
            let from = self.from + (window * index).div_ceil(count);
            let to = self.from + (window * (index + 1)).div_ceil(count);
            let value = value.map_or_else(String::new, |value| value.to_string());
            writeln!(
                csv,
                "{from},{to},{value},{},{},{}\r",
                self.unit, self.partial, self.partial_bars[index as usize]
            )
            .expect("writing to a String");
        }
        csv
    }

    pub fn estimated_cost(&self, meter: Meter, rates: Rates) -> Option<f64> {
        match meter {
            Meter::Electricity => {
                let [standard, off_peak] = self.tariff_usage;
                Some(
                    (standard? * f64::from(rates.standard?)
                        + off_peak? * f64::from(rates.off_peak?))
                        / 1000.0,
                )
            }
            Meter::Gas => Some(self.total? * f64::from(rates.gas?) / 1000.0),
            _ => None,
        }
    }
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
    fn csv_preserves_zero_and_missing_buckets_with_chart_boundaries() {
        let chart = Chart {
            bars: vec![Some(0.0), None, Some(1.25)],
            partial_bars: vec![false, true, true],
            unit: "kWh",
            total: Some(1.25),
            partial: true,
            from: 100,
            to: 110,
            tariff_usage: [None; 2],
        };
        assert_eq!(
            chart.csv(),
            "from_unix,to_unix,value,unit,window_partial,bucket_partial\r\n100,104,0,kWh,true,false\r\n104,107,,kWh,true,true\r\n107,110,1.25,kWh,true,true\r\n"
        );
    }

    #[test]
    fn rates_preserve_precision_and_missing_values() {
        let mut rates = Rates::default();
        Rate::Standard.save(&mut rates, "0,355").unwrap();
        Rate::OffPeak.save(&mut rates, "0.25").unwrap();
        Rate::Gas.save(&mut rates, "1.2").unwrap();
        assert!(Rate::Gas.save(&mut rates, "1.2345").is_err());
        let chart = Chart {
            bars: Vec::new(),
            partial_bars: Vec::new(),
            unit: "kWh",
            total: Some(3.0),
            partial: true,
            from: 0,
            to: 3600,
            tariff_usage: [Some(2.0), Some(1.0)],
        };
        assert_eq!(chart.estimated_cost(Meter::Electricity, rates), Some(0.96));
        assert_eq!(rates.gas, Some(1200));
        Rate::OffPeak.save(&mut rates, "").unwrap();
        assert_eq!(chart.estimated_cost(Meter::Electricity, rates), None);
    }

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
    // Include a counter baseline even when the firmware treats "from" as exclusive.
    let query_start = if period.flow() {
        start
    } else {
        start.saturating_sub(interval)
    };
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
    let mut readings = samples(primary, archive, query_start, end, count)?;
    let mut partial = false;
    let mut tariff_usage = [None; 2];
    if !period.flow() {
        readings = changes(readings, interval);
        if let Some(extra) = extra {
            let secondary = changes(samples(extra, archive, query_start, end, count)?, interval);
            let has_readings = |samples: &Samples| {
                samples
                    .range((start + 1)..=end)
                    .any(|(time, value)| (time - start).is_multiple_of(interval) && value.is_some())
            };
            let primary_available = has_readings(&readings);
            let secondary_available = has_readings(&secondary);
            if !primary_available || !secondary_available {
                partial = true;
                if !primary_available {
                    readings = secondary;
                }
            } else {
                for (&time, value) in &mut readings {
                    *value = value
                        .zip(secondary.get(&time).copied().flatten())
                        .map(|(a, b)| {
                            if meter == Meter::Electricity
                                && time > start
                                && time <= end
                                && (time - start).is_multiple_of(interval)
                            {
                                *tariff_usage[0].get_or_insert(0.0) += a;
                                *tariff_usage[1].get_or_insert(0.0) += b;
                            }
                            a + b
                        });
                }
            }
        }
    }
    let mut sums = vec![(0.0, 0u32); buckets];
    let mut observed = 0;
    for (time, reading) in readings {
        if time <= start || time > end {
            continue;
        }
        let Some(reading) = reading else {
            continue;
        };
        if !(time - start).is_multiple_of(interval) {
            continue;
        }
        let index = (((time - start - 1) * buckets as u64) / window) as usize;
        sums[index].0 += reading;
        sums[index].1 += 1;
        observed += 1;
    }
    let sum: f64 = sums.iter().map(|(sum, _)| *sum).sum();
    let total = (!period.flow() && observed > 0).then_some(sum);
    let expected_before =
        |bucket: usize| (window * bucket as u64).div_ceil(buckets as u64) / interval;
    let partial_bars = sums
        .iter()
        .enumerate()
        .map(|(index, (_, count))| {
            partial || u64::from(*count) < expected_before(index + 1) - expected_before(index)
        })
        .collect();
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
        partial_bars,
        unit: meter.unit(period),
        total,
        partial: partial || observed < window / interval,
        from: start,
        to: end,
        tariff_usage,
    })
}
