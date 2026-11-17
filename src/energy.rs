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
    pub(crate) tariff_usage: [Option<f64>; 2],
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
    pub fn bucket_bounds(&self, index: usize) -> (u64, u64) {
        let count = self.bars.len() as u64;
        let window = self.to - self.from;
        let boundary = |i: usize| self.from + (window * i as u64).div_ceil(count);
        (boundary(index), boundary(index + 1))
    }

    pub fn csv(&self) -> String {
        use std::fmt::Write as _;
        let mut csv =
            String::from("from_unix,to_unix,value,unit,window_partial,bucket_partial\r\n");
        for (index, value) in self.bars.iter().enumerate() {
            let (from, to) = self.bucket_bounds(index);
            let value = value.map_or_else(String::new, |value| value.to_string());
            writeln!(
                csv,
                "{from},{to},{value},{},{},{}\r",
                self.unit, self.partial, self.partial_bars[index]
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
    fn gas_window_reports_zero_after_the_last_counter_change_leaves_the_window() {
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let mut window = GasWindow::default();
        for second in (0..=610).step_by(10) {
            let result = window.observe(
                start + Duration::from_secs(second),
                Some(if second < 300 { 100.0 } else { 104.0 }),
            );
            match second {
                300 => assert_eq!(result, Some((4.0, 300))),
                600 => assert_eq!(result, Some((0.0, 300))),
                _ => {}
            }
        }
        assert_eq!(
            window.observe(start + Duration::from_secs(620), Some(1.0)),
            None
        );
        assert_eq!(
            window.observe(start + Duration::from_secs(670), Some(1.0)),
            None
        );
    }

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

    #[test]
    fn selected_meter_history_produces_usage_and_keeps_missing_intervals() {
        let from = 1_800_000_000;
        let samples = [
            (from, Some(1000.0)),
            (from + METER_STEP, Some(1250.0)),
            (from + 2 * METER_STEP, Some(1250.0)),
            (from + 4 * METER_STEP, Some(1500.0)),
        ]
        .into();
        let chart = build_chart(
            Meter::Electricity,
            Period::Day,
            changes(samples, METER_STEP),
            METER_STEP,
            from,
            false,
            [None; 2],
        );
        assert_eq!(chart.bars[0], Some(0.25));
        assert_eq!(chart.total, Some(0.25));
        assert!(chart.partial_bars[0]);
        assert_eq!(chart.bars[1], None);
    }
}

pub fn load(meter: Meter, period: Period) -> Result<Chart, String> {
    let (mut archive, window, mut interval, _) = period.query();
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
    Ok(build_chart(
        meter,
        period,
        readings,
        interval,
        start,
        partial,
        tariff_usage,
    ))
}

fn build_chart(
    meter: Meter,
    period: Period,
    readings: Samples,
    interval: u64,
    start: u64,
    partial: bool,
    tariff_usage: [Option<f64>; 2],
) -> Chart {
    let (_, window, _, buckets) = period.query();
    let end = start + window;
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
    Chart {
        bars,
        partial_bars,
        unit: meter.unit(period),
        total,
        partial: partial || observed < window / interval,
        from: start,
        to: end,
        tariff_usage,
    }
}

const METER_STEP: u64 = 300;

#[derive(Default)]
pub struct GasWindow {
    samples: std::collections::VecDeque<(std::time::Instant, f64)>,
}

impl GasWindow {
    pub fn observe(&mut self, now: std::time::Instant, counter: Option<f64>) -> Option<(f64, u64)> {
        let Some(counter) = counter.filter(|value| value.is_finite() && *value >= 0.0) else {
            self.samples.clear();
            return None;
        };
        if self.samples.back().is_some_and(|(at, previous)| {
            counter < *previous || now.duration_since(*at).as_secs() > 35
        }) {
            self.samples.clear();
        }
        self.samples.push_back((now, counter));
        while self
            .samples
            .get(1)
            .is_some_and(|(at, _)| now.duration_since(*at).as_secs() >= METER_STEP)
        {
            self.samples.pop_front();
        }
        let &(at, baseline) = self.samples.front()?;
        let seconds = now.duration_since(at).as_secs();
        (seconds >= METER_STEP).then_some((counter - baseline, seconds))
    }
}

impl Meter {
    pub fn selected_metric(self) -> Option<crate::plugins::Metric> {
        use crate::plugins::Metric;
        match self {
            Self::Electricity => Some(Metric::Electricity),
            Self::Gas => Some(Metric::Gas),
            _ => None,
        }
    }
}

fn selected_path(
    root: &std::path::Path,
    metric: crate::plugins::Metric,
    source: &str,
) -> std::path::PathBuf {
    use sha2::{Digest, Sha256};
    let key = Sha256::digest(format!("{metric:?}:{source}").as_bytes());
    root.join(format!("meter-{key:x}.csv"))
}

pub fn record_selected(
    root: &std::path::Path,
    metric: crate::plugins::Metric,
    source: &str,
    combined_import: bool,
    flow: Option<f64>,
    total: Option<f64>,
) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    if flow.is_none() && total.is_none() {
        return Ok(());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_secs();
    if now < 1_577_836_800 {
        return Ok(());
    }
    let time = now - now % METER_STEP;
    let combined_source;
    let source = if combined_import {
        combined_source = format!("{source}:combined-import-v1");
        combined_source.as_str()
    } else {
        source
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(selected_path(root, metric, source))?;
    let length = file.metadata()?.len();
    if length != 0 {
        let mut tail = Vec::new();
        file.seek(SeekFrom::Start(length.saturating_sub(128)))?;
        file.read_to_end(&mut tail)?;
        if let Some(previous) = String::from_utf8_lossy(&tail)
            .lines()
            .last()
            .and_then(|line| line.split_once(','))
            .and_then(|(time, _)| time.parse::<u64>().ok())
        {
            if tail.last() == Some(&b'\n') && previous >= time {
                return Ok(());
            }
        }
        if tail.last() != Some(&b'\n') {
            file.write_all(b"\n")?;
        }
    }
    let value = |value: Option<f64>| {
        value
            .filter(|x| x.is_finite())
            .map(|x| x.to_string())
            .unwrap_or_default()
    };
    writeln!(file, "{time},{},{}", value(flow), value(total))?;
    file.sync_data()
}

pub fn load_selected(
    root: &std::path::Path,
    meter: Meter,
    period: Period,
    source: Option<&str>,
) -> Result<Chart, String> {
    use std::io::{BufRead, BufReader, ErrorKind};
    let (Some(metric), Some(source)) = (meter.selected_metric(), source) else {
        return load(meter, period);
    };
    let (_, window, _, _) = period.query();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let end = now - now % METER_STEP;
    let start = end.saturating_sub(window);
    let mut readings = Samples::new();
    let archive = if meter == Meter::Electricity {
        match std::fs::File::open(selected_path(
            root,
            metric,
            &format!("{source}:combined-import-v1"),
        )) {
            Err(error) if error.kind() == ErrorKind::NotFound => {
                std::fs::File::open(selected_path(root, metric, source))
            }
            result => result,
        }
    } else {
        std::fs::File::open(selected_path(root, metric, source))
    };
    match archive {
        Ok(file) => {
            for line in BufReader::new(file).lines() {
                let line = line.map_err(|e| e.to_string())?;
                let mut fields = line.split(',');
                let (Some(time), Some(flow), Some(total), None) = (
                    fields.next().and_then(|x| x.parse::<u64>().ok()),
                    fields.next(),
                    fields.next(),
                    fields.next(),
                ) else {
                    continue;
                };
                if !(start..=end).contains(&time) {
                    continue;
                }
                let value = (if period.flow() && meter != Meter::Gas {
                    flow
                } else {
                    total
                })
                .parse::<f64>()
                .ok()
                .filter(|x| x.is_finite() && (period.flow() || *x >= 0.0));
                readings.insert(time, value);
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    if meter == Meter::Gas && period.flow() {
        readings = changes(readings, METER_STEP);
        for value in readings.values_mut().flatten() {
            *value *= 3_600_000.0 / METER_STEP as f64;
        }
    } else if !period.flow() {
        readings = changes(readings, METER_STEP);
    }
    Ok(build_chart(
        meter, period, readings, METER_STEP, start, false, [None; 2],
    ))
}
