use crate::thermostat::Status;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Default)]
pub struct Inputs {
    pub enabled: bool,
    pub reset: u64,
    pub open_therm: Option<bool>,
    pub module: Option<bool>,
    pub calibration: Option<i32>,
    pub gas: Option<(f64, Instant)>,
}

pub type Shared = Arc<Mutex<Inputs>>;

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Summary {
    pub advice: &'static str,
    pub cooling_samples: usize,
    pub warming_samples: usize,
    pub residual_samples: usize,
    pub calls: usize,
    pub short_calls: usize,
    pub median_seconds: Option<u64>,
    pub below_seconds: u64,
    pub burner_stops_under_demand: usize,
    pub calls_per_heating_hour: Option<f64>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct History {
    #[serde(default)]
    protocol: Option<bool>,
    #[serde(default)]
    calibration: Option<i32>,
    cooling: VecDeque<i32>,
    warming: VecDeque<i32>,
    residual: VecDeque<i32>,
}

impl History {
    fn add(rows: &mut VecDeque<i32>, value: i32) {
        while rows.len() >= 8 {
            rows.pop_front();
        }
        rows.push_back(value);
    }

    fn average(rows: &VecDeque<i32>) -> Option<i32> {
        (rows.len() >= 5).then(|| rows.iter().sum::<i32>() / rows.len() as i32)
    }
}

#[derive(Clone, Copy)]
struct Reading {
    room: i32,
    target: i32,
    call: bool,
    burner: Option<bool>,
    scheduled: bool,
    next: u64,
    preset: i32,
}

impl Reading {
    fn parse(status: &Status, input: &Inputs) -> Option<Self> {
        if status.heating_error() == Some(true)
            || input.module == Some(false)
            || (input.open_therm == Some(true) && status.ot_comm_error != "0")
        {
            return None;
        }
        let drive = status
            .current_modulation_level
            .parse::<u8>()
            .ok()
            .filter(|value| *value <= 100)?;
        let flow_target = status
            .current_internal_boiler_setpoint
            .parse::<i32>()
            .ok()?;
        let call = drive > 0 || flow_target > 6;
        let burner = if input.open_therm == Some(true) && input.module == Some(true) {
            match status.burner_info.as_str() {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            }
        } else {
            None
        };
        Some(Self {
            room: status
                .current_temp
                .parse::<i32>()
                .ok()
                .filter(|value| (-1000..=5000).contains(value))?,
            target: status
                .target()
                .filter(|value| (500..=3500).contains(value))?,
            call,
            burner,
            scheduled: status.program_state == "1"
                && matches!(status.active_state.as_str(), "0" | "1"),
            next: status.next_time.parse().unwrap_or(0),
            preset: status.active_state.parse().unwrap_or(-1),
        })
    }

    fn same_period(self, other: Self) -> bool {
        self.target == other.target
            && self.scheduled == other.scheduled
            && self.next == other.next
            && self.preset == other.preset
    }
}

struct Segment {
    start: Instant,
    initial: Reading,
    maximum: i32,
    residual: bool,
    observed_start: bool,
}

#[derive(Serialize)]
struct Trace {
    timestamp: u64,
    room_centi_c: Option<i32>,
    target_centi_c: Option<i32>,
    inferred_heat_call: Option<bool>,
    confirmed_heating_burner: Option<bool>,
    scheduled: Option<bool>,
    gas_counter_liters: Option<f64>,
    open_therm: Option<bool>,
}

enum Save {
    Trace(Trace),
    History(History),
}

pub struct Observer {
    history: History,
    storage: std::sync::mpsc::SyncSender<Save>,
    last: Option<(Instant, Reading)>,
    last_trace: Option<Instant>,
    segment: Option<Segment>,
    calls: VecDeque<u64>,
    below_seconds: u64,
    stops: usize,
    reset: u64,
    last_advice: Option<(Instant, &'static str)>,
}

impl Observer {
    pub fn new(root: PathBuf) -> Self {
        let path = root.join("heating-observations.json");
        let history: History = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<History>(&bytes).ok())
            .filter(|data| {
                [&data.cooling, &data.warming, &data.residual]
                    .iter()
                    .all(|rows| {
                        rows.len() <= 8 && rows.iter().all(|n| (-10_000..=10_000).contains(n))
                    })
            })
            .unwrap_or_default();
        let (storage, tasks) = std::sync::mpsc::sync_channel(32);
        std::thread::spawn(move || {
            while let Ok(task) = tasks.recv() {
                let result = match task {
                    Save::Trace(trace) => write_trace(&root, &trace),
                    Save::History(history) => write_history(&path, &history),
                };
                if let Err(error) = result {
                    eprintln!("Heating diagnostics: {error}");
                }
            }
        });
        Self::with_history(history, storage)
    }

    fn with_history(history: History, storage: std::sync::mpsc::SyncSender<Save>) -> Self {
        Self {
            history,
            storage,
            last: None,
            last_trace: None,
            segment: None,
            calls: VecDeque::new(),
            below_seconds: 0,
            stops: 0,
            reset: 0,
            last_advice: None,
        }
    }

    pub fn observe(&mut self, now: Instant, status: Option<&Status>, input: &Inputs) -> Summary {
        let protocol = input.open_therm.or(self.history.protocol);
        let calibration = input.calibration.or(self.history.calibration);
        let changed_protocol = self.history.protocol.is_some() && protocol != self.history.protocol;
        let changed_calibration =
            self.history.calibration.is_some() && calibration != self.history.calibration;
        let reset = input.reset != self.reset || changed_protocol || changed_calibration;
        if reset {
            self.history = History::default();
            self.calls.clear();
            self.below_seconds = 0;
            self.stops = 0;
            self.last = None;
            self.segment = None;
            self.last_advice = None;
            self.reset = input.reset;
        }
        self.history.protocol = protocol;
        self.history.calibration = calibration;
        if reset {
            self.save_history();
        }
        if !input.enabled {
            self.last = None;
            self.segment = None;
            self.last_advice = None;
            return self.summary("Firmware is controlling heating");
        }
        let reading = status.and_then(|status| Reading::parse(status, input));
        let previous = self
            .last
            .filter(|(at, _)| now.duration_since(*at) <= Duration::from_secs(12));
        let changed = match (previous, reading) {
            (Some((_, prev)), Some(next)) => {
                prev.call != next.call || prev.burner != next.burner || !prev.same_period(next)
            }
            _ => self.last.is_some() != reading.is_some(),
        };
        if changed
            || self
                .last_trace
                .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(15))
        {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let trace = Trace {
                timestamp,
                room_centi_c: reading.map(|r| r.room),
                target_centi_c: reading.map(|r| r.target),
                inferred_heat_call: reading.map(|r| r.call),
                confirmed_heating_burner: reading.and_then(|r| r.burner),
                scheduled: reading.map(|r| r.scheduled),
                open_therm: input.open_therm,
                gas_counter_liters: input
                    .gas
                    .filter(|(_, at)| now.saturating_duration_since(*at) <= Duration::from_secs(30))
                    .map(|(gas, _)| gas),
            };
            let _ = self.storage.try_send(Save::Trace(trace));
            self.last_trace = Some(now);
        }
        let Some(reading) = reading else {
            self.last = None;
            self.segment = None;
            self.last_advice = None;
            return self.summary("Waiting for valid thermostat readings");
        };
        let comparable = previous.filter(|(_, previous)| {
            previous.same_period(reading) && (reading.room - previous.room).abs() <= 30
        });
        if let Some((at, prev)) = comparable {
            if prev.call
                && reading.call
                && prev.burner == Some(true)
                && reading.burner == Some(false)
            {
                self.stops += 1;
            }
            if reading.scheduled && reading.room < reading.target - 50 {
                self.below_seconds += now.duration_since(at).as_secs();
            }
        } else {
            self.segment = None;
            self.last_advice = None;
        }
        self.last = Some((now, reading));
        let old = self.segment.take();
        self.segment = Some(match old {
            Some(mut segment) if segment.initial.call == reading.call => {
                segment.maximum = segment.maximum.max(reading.room);
                if segment.residual && now.duration_since(segment.start) >= Duration::from_secs(300)
                {
                    History::add(
                        &mut self.history.residual,
                        segment.maximum - segment.initial.room,
                    );
                    self.save_history();
                    Segment {
                        start: now,
                        initial: reading,
                        maximum: reading.room,
                        residual: false,
                        observed_start: false,
                    }
                } else {
                    segment
                }
            }
            Some(segment) => {
                let elapsed = now.duration_since(segment.start).as_secs();
                if segment.initial.call && segment.observed_start && elapsed > 0 {
                    if self.calls.len() >= 128 {
                        self.calls.pop_front();
                    }
                    self.calls.push_back(elapsed);
                }
                if reading.scheduled && elapsed >= 120 {
                    let change = reading.room - segment.initial.room;
                    let rate = (i64::from(change) * 3600 / elapsed as i64) as i32;
                    let destination = if segment.initial.call
                        && segment.observed_start
                        && (1..=10_000).contains(&rate)
                    {
                        Some(&mut self.history.warming)
                    } else if !segment.residual
                        && !segment.initial.call
                        && (-10_000..=-1).contains(&rate)
                    {
                        Some(&mut self.history.cooling)
                    } else {
                        None
                    };
                    if let Some(rows) = destination {
                        History::add(rows, rate);
                        self.save_history();
                    }
                }
                self.last_advice = None;
                Segment {
                    start: now,
                    initial: reading,
                    maximum: reading.room,
                    residual: !reading.call && reading.scheduled,
                    observed_start: true,
                }
            }
            None => Segment {
                start: now,
                initial: reading,
                maximum: reading.room,
                residual: false,
                observed_start: false,
            },
        });
        let advice = if !reading.scheduled {
            "Learning advice requires a Home or Comfort schedule period"
        } else if let Some(segment) = &self.segment {
            let elapsed = now.duration_since(segment.start).as_secs();
            let cooling = History::average(&self.history.cooling);
            if reading.call
                && segment.observed_start
                && elapsed < 300
                && cooling
                    .is_some_and(|rate| reading.room - ((-rate + 59) / 60) > reading.target - 50)
            {
                if self
                    .last_advice
                    .is_none_or(|(at, _)| now.duration_since(at) >= Duration::from_secs(60))
                {
                    self.last_advice = Some((now, "Could coast for another minute (advice only)"));
                }
                self.last_advice.map_or("", |(_, advice)| advice)
            } else if reading.call
                && reading.room < reading.target
                && History::average(&self.history.residual)
                    .is_some_and(|rise| rise > 0 && reading.room + rise >= reading.target)
            {
                "Residual heat may reach the target (advice only)"
            } else if self.history.cooling.len() < 5 || self.history.warming.len() < 5 {
                "Collecting qualified heating observations"
            } else {
                "Firmware is controlling heating"
            }
        } else {
            "Collecting qualified heating observations"
        };
        self.summary(advice)
    }

    fn save_history(&self) {
        let _ = self.storage.try_send(Save::History(self.history.clone()));
    }

    fn summary(&self, advice: &'static str) -> Summary {
        let mut durations: Vec<_> = self.calls.iter().copied().collect();
        durations.sort_unstable();
        let total = durations.iter().sum::<u64>();
        Summary {
            advice,
            cooling_samples: self.history.cooling.len(),
            warming_samples: self.history.warming.len(),
            residual_samples: self.history.residual.len(),
            calls: durations.len(),
            short_calls: durations.iter().filter(|&&time| time < 180).count(),
            median_seconds: durations.get(durations.len() / 2).copied(),
            below_seconds: self.below_seconds,
            burner_stops_under_demand: self.stops,
            calls_per_heating_hour: (total > 0)
                .then(|| durations.len() as f64 * 3600.0 / total as f64),
        }
    }
}

fn write_history(path: &Path, history: &History) -> std::io::Result<()> {
    use std::io::Write;
    let temp = path.with_extension("new");
    let mut file = std::fs::File::create(&temp)?;
    serde_json::to_writer(&mut file, history)?;
    file.flush()?;
    file.sync_all()?;
    std::fs::rename(temp, path)
}

fn write_trace(root: &Path, trace: &Trace) -> std::io::Result<()> {
    use std::io::Write;
    let day = trace.timestamp / 86_400;
    if day < 18_000 {
        return Ok(());
    }
    let path = root.join(format!("heating-trace-{day}.jsonl"));
    if !path.exists() {
        for entry in std::fs::read_dir(root)?.flatten() {
            let name = entry.file_name();
            let Some(outdated) = name
                .to_str()
                .and_then(|name| name.strip_prefix("heating-trace-"))
                .and_then(|name| name.strip_suffix(".jsonl"))
                .and_then(|name| name.parse::<u64>().ok())
            else {
                continue;
            };
            if outdated.saturating_add(30) <= day {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    let mut row = serde_json::to_vec(trace)?;
    row.push(b'\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(&row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observer() -> (Observer, std::sync::mpsc::Receiver<Save>) {
        let (send, receive) = std::sync::mpsc::sync_channel(32);
        (Observer::with_history(History::default(), send), receive)
    }

    fn status(room: i32, demand: bool) -> Status {
        serde_json::from_value(serde_json::json!({
            "currentTemp":room.to_string(),"currentSetpoint":"2000","programState":"1",
            "activeState":"1","burnerInfo":"1","connection":"0","nextSetpoint":"1700",
            "nextTime":"1234","errorFound":"255","otCommError":"0",
            "currentInternalBoilerSetpoint":if demand {"80"} else {"0"},
            "currentModulationLevel":if demand {"30"} else {"0"}
        }))
        .unwrap()
    }

    #[test]
    fn coasting_advice_uses_history_and_the_comfort_floor() {
        let (mut observer, _storage) = observer();
        let input = Inputs {
            enabled: true,
            open_therm: Some(false),
            ..Inputs::default()
        };
        let now = Instant::now();
        observer.history.cooling = VecDeque::from([-60; 5]);
        observer.observe(now, Some(&status(1956, false)), &input);
        let report = observer.observe(
            now + Duration::from_secs(3),
            Some(&status(1955, true)),
            &input,
        );
        assert!(report.advice.starts_with("Could coast"));
        observer.history.cooling = VecDeque::from([-600; 5]);
        let report = observer.observe(
            now + Duration::from_secs(6),
            Some(&status(1955, true)),
            &input,
        );
        assert!(!report.advice.starts_with("Could coast"));
    }

    #[test]
    fn never_counts_a_partial_call_or_relay_burner_feedback() {
        let (mut observer, _storage) = observer();
        let input = Inputs {
            enabled: true,
            open_therm: Some(false),
            module: Some(true),
            ..Inputs::default()
        };
        let now = Instant::now();
        observer.observe(now, Some(&status(1955, true)), &input);
        let mut off = status(1960, false);
        off.burner_info = "0".into();
        assert_eq!(
            observer
                .observe(now + Duration::from_secs(3), Some(&off), &input)
                .calls,
            0
        );
        observer.observe(
            now + Duration::from_secs(6),
            Some(&status(1955, true)),
            &input,
        );
        let report = observer.observe(now + Duration::from_secs(9), Some(&off), &input);
        assert_eq!(
            (
                report.calls,
                report.short_calls,
                report.burner_stops_under_demand
            ),
            (1, 1, 0)
        );
    }

    #[test]
    fn resets_learning_on_calibration_and_breaks_segments_on_schedule_change_or_gap() {
        let (mut observer, _storage) = observer();
        let mut input = Inputs {
            enabled: true,
            open_therm: Some(false),
            calibration: Some(0),
            ..Inputs::default()
        };
        let now = Instant::now();
        observer.history.cooling = VecDeque::from([-60; 5]);
        observer.observe(now, Some(&status(1956, false)), &input);
        observer.observe(
            now + Duration::from_secs(3),
            Some(&status(1955, true)),
            &input,
        );
        let mut changed = status(1956, false);
        changed.next_time = "1300".into();
        assert_eq!(
            observer
                .observe(now + Duration::from_secs(6), Some(&changed), &input)
                .calls,
            0
        );
        observer.observe(
            now + Duration::from_secs(35),
            Some(&status(1957, true)),
            &input,
        );
        input.calibration = Some(20);
        assert_eq!(
            observer
                .observe(now + Duration::from_secs(38), Some(&changed), &input)
                .cooling_samples,
            0
        );
    }
}
