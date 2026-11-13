use serde::Deserialize;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

#[derive(Debug, Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub current_temp: String,
    pub current_setpoint: String,
    pub program_state: String,
    pub burner_info: String,
    pub connection: String,
    pub next_setpoint: String,
    pub next_time: String,
    #[serde(default)]
    pub current_internal_boiler_setpoint: String,
    #[serde(default)]
    pub current_modulation_level: String,
    #[serde(default)]
    pub ot_comm_error: String,
}

pub struct History {
    pub room: Vec<Option<f64>>,
    pub target: Vec<Option<f64>>,
    pub raw_room: Option<f64>,
    pub diagnostics: Option<Diagnostics>,
}

#[derive(serde::Serialize)]
pub struct Diagnostics {
    pub firmware_version: Option<String>,
    pub calibration_offset_celsius: Option<f64>,
    pub measured_centi_celsius: Option<i32>,
    pub open_therm_configured: Option<bool>,
    pub hot_water_enabled: Option<bool>,
    pub hot_water_target_celsius: Option<i32>,
}

pub fn diagnostics() -> Result<Diagnostics, String> {
    use crate::boxtalk::field;
    let replies = crate::boxtalk::thermostat(&["GetTempOffset", "GetBoilerType", "GetDhwSettings"])
        .map_err(|error| error.to_string())?;
    let version = firmware_get("hcb_config", "action=getVersionInfo.json")
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .and_then(|value| value["firmwareVersion"].as_str().map(str::to_owned));
    let flag = |value| match value {
        Some("0") => Some(false),
        Some("1") => Some(true),
        _ => None,
    };
    Ok(Diagnostics {
        firmware_version: version,
        calibration_offset_celsius: field(&replies[0], "offset")
            .and_then(|v| v.parse().ok())
            .filter(|v: &f64| v.is_finite()),
        measured_centi_celsius: field(&replies[0], "measuredTemp").and_then(|v| v.parse().ok()),
        open_therm_configured: flag(field(&replies[1], "ot")),
        hot_water_enabled: flag(field(&replies[2], "dhwEnabled")),
        hot_water_target_celsius: field(&replies[2], "dhwSetpoint").and_then(|v| v.parse().ok()),
    })
}

#[cfg(target_os = "linux")]
pub fn export_diagnostics(root: &std::path::Path) -> std::io::Result<()> {
    use std::fs::{self, File, OpenOptions};
    use std::os::unix::fs::OpenOptionsExt;

    let diagnostics = diagnostics().ok();
    let status = status().ok();
    let report = serde_json::json!({
        "toonshift_version": env!("CARGO_PKG_VERSION"),
        "firmware_version": diagnostics.as_ref().and_then(|d| d.firmware_version.as_deref())
            .filter(|value| value.len() <= 32 && value.bytes().all(|c| c.is_ascii_digit() || c == b'.')),
        "open_therm_configured": diagnostics.as_ref().and_then(|d| d.open_therm_configured),
        "hot_water_enabled": diagnostics.as_ref().and_then(|d| d.hot_water_enabled),
        "thermostat_responded": status.is_some(),
        "boiler_connected": status.as_ref().and_then(|s| match s.connection.as_str() {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        }),
        "open_therm_communication_error": status.as_ref().and_then(|s| match s.ot_comm_error.as_str() {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        }),
    });
    let temporary = root.join("diagnostics.json.tmp");
    match fs::remove_file(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, root.join("diagnostics.json"))?;
        File::open(root)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub fn history() -> Result<History, String> {
    let response = firmware_get("happ_thermstat", "action=getHeatingHistory")?;
    let parse = |name: &str| -> Result<Vec<Option<f64>>, String> {
        let marker = format!("{name}:[");
        let (_, body) = response
            .split_once(&marker)
            .ok_or("Missing temperature series")?;
        let (series, _) = body
            .split_once(']')
            .ok_or("Incomplete temperature series")?;
        Ok(series
            .split(',')
            .filter(|value| !value.trim().is_empty())
            .take(1024)
            .map(|value| {
                value.trim().parse::<f64>().ok().filter(|value| {
                    value.is_finite() && (0.0..=60.0).contains(value) && *value != 0.0
                })
            })
            .collect())
    };
    Ok(History {
        room: parse("roomTemps")?,
        target: parse("setpoints")?,
        raw_room: parse("uncorrectedRoomtemps")
            .ok()
            .and_then(|values| values.into_iter().flatten().last()),
        diagnostics: diagnostics().ok(),
    })
}

impl Status {
    pub fn target(&self) -> Option<i32> {
        self.current_setpoint.parse().ok()
    }

    pub fn heat_requested(&self) -> bool {
        self.target()
            .zip(self.current_temp.parse::<i32>().ok())
            .is_some_and(|(target, room)| target > room)
            && (self
                .current_modulation_level
                .parse::<u8>()
                .is_ok_and(|level| level > 0)
                || self
                    .current_internal_boiler_setpoint
                    .parse::<i32>()
                    .is_ok_and(|target| target > 6))
    }
}

#[derive(Clone, Copy)]
pub enum Command {
    Adjust(i32),
    Setpoint(i32),
    Program(bool),
    Schedule,
    EditSchedule(crate::schedule::Edit),
    EditScheduleAt(crate::schedule::Edit, u64),
    Presets,
    SavePreset(crate::presets::Preset),
    ApplyPreset(u8),
}

pub struct Work {
    command: Command,
    reply: Option<Sender<Result<(), String>>>,
}

impl Work {
    pub fn from_ui(command: Command) -> Self {
        Self {
            command,
            reply: None,
        }
    }

    pub fn from_api(command: Command, reply: Sender<Result<(), String>>) -> Self {
        Self {
            command,
            reply: Some(reply),
        }
    }
}

pub struct Update {
    pub status: Option<Status>,
    pub error: Option<String>,
    pub completed: bool,
    pub schedule: Option<Result<Vec<crate::schedule::Slot>, String>>,
    pub presets: Option<Result<Vec<crate::presets::Preset>, String>>,
}

pub fn firmware_get(service: &str, query: &str) -> Result<String, String> {
    let timeout = Duration::from_secs(if service == "hcb_netcon" { 6 } else { 2 });
    firmware_get_timeout(service, query, timeout)
}

pub fn firmware_get_timeout(
    service: &str,
    query: &str,
    timeout: Duration,
) -> Result<String, String> {
    fn send(service: &str, query: &str, timeout: Duration) -> std::io::Result<String> {
        let address = SocketAddr::from(([127, 0, 0, 1], 10080));
        let mut stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        write!(
            stream,
            "GET /{service}?{query} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )?;
        let mut response = String::new();
        let limit = if matches!(service, "hcb_rrd" | "hdrv_zwave") {
            131072
        } else {
            32768
        };
        stream.take(limit).read_to_string(&mut response)?;
        Ok(response)
    }
    let response = send(service, query, timeout).map_err(|err| err.to_string())?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or("Incomplete HTTP response")?;
    if headers.split_whitespace().nth(1) != Some("200") {
        return Err("Firmware HTTP request failed".into());
    }
    Ok(body.into())
}

fn request(query: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value = serde_json::from_str(&firmware_get("happ_thermstat", query)?)
        .map_err(|err| err.to_string())?;
    if value["result"] != "ok" {
        return Err(value["error"]
            .as_str()
            .unwrap_or("Thermostat rejected the request")
            .into());
    }
    Ok(value)
}

pub fn status() -> Result<Status, String> {
    serde_json::from_value(request("action=getThermostatInfo")?).map_err(|err| err.to_string())
}

fn apply(command: Command) -> Result<(), String> {
    let query = match command {
        Command::Adjust(delta) => {
            let current = status()?.target().ok_or("Invalid thermostat setpoint")?;
            let target = (current + delta).clamp(600, 3000);
            format!("action=setSetpoint&Setpoint={target}")
        }
        Command::Setpoint(target) if (600..=3000).contains(&target) => {
            format!("action=setSetpoint&Setpoint={target}")
        }
        Command::Setpoint(_) => return Err("Setpoint must be between 6 and 30°C".into()),
        Command::Program(enabled) => {
            format!("action=changeSchemeState&state={}", u8::from(enabled))
        }
        Command::ApplyPreset(4) => "action=changeSchemeState&state=8&temperatureState=4".into(),
        Command::ApplyPreset(preset) if preset < 4 => {
            if status()?.program_state == "0" {
                let temperature = crate::presets::load()?
                    .into_iter()
                    .find(|row| row.id == preset)
                    .ok_or("Preset is unavailable")?
                    .temperature;
                format!("action=setSetpoint&Setpoint={temperature}")
            } else {
                format!("action=changeSchemeState&state=2&temperatureState={preset}")
            }
        }
        Command::ApplyPreset(_) => return Err("Invalid preset".into()),
        Command::Schedule
        | Command::EditSchedule(_)
        | Command::EditScheduleAt(_, _)
        | Command::Presets
        | Command::SavePreset(_) => return Ok(()),
    };
    request(&query)?;
    Ok(())
}

pub fn worker() -> (Sender<Work>, Receiver<Update>) {
    let (send_command, commands) = mpsc::channel::<Work>();
    let (send_update, updates) = mpsc::channel();
    std::thread::spawn(move || {
        let mut work: Option<Work> = None;
        loop {
            let command = work.as_ref().map(|work| work.command);
            let schedule = match command {
                Some(Command::Schedule) => Some(crate::schedule::load()),
                Some(Command::EditSchedule(edit)) => Some(crate::schedule::edit(edit, None)),
                Some(Command::EditScheduleAt(edit, revision)) => {
                    Some(crate::schedule::edit(edit, Some(revision)))
                }
                _ => None,
            };
            let presets = match command {
                Some(Command::Presets) => Some(crate::presets::load()),
                Some(Command::SavePreset(preset)) => Some(crate::presets::save(preset)),
                _ => None,
            };
            let heat = command.filter(|command| {
                !matches!(
                    command,
                    Command::Schedule
                        | Command::EditSchedule(_)
                        | Command::EditScheduleAt(_, _)
                        | Command::Presets
                        | Command::SavePreset(_)
                )
            });
            let result = heat.map(apply).transpose();
            let reply = work.take().and_then(|work| work.reply);
            let completed = heat.is_some() && reply.is_none();
            if let Some(reply) = reply {
                let response = match &schedule {
                    Some(schedule) => schedule.as_ref().map(|_| ()).map_err(Clone::clone),
                    None => result.clone().map(|_| ()),
                };
                let _ = reply.send(response);
            }
            let (status, error) = match status() {
                Ok(value) => (Some(value), result.err()),
                Err(error) => (None, Some(result.err().unwrap_or(error))),
            };
            if send_update
                .send(Update {
                    status,
                    error,
                    completed,
                    schedule,
                    presets,
                })
                .is_err()
            {
                break;
            }
            work = match commands.recv_timeout(Duration::from_secs(3)) {
                Ok(work) => Some(work),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
        }
    });
    (send_command, updates)
}
