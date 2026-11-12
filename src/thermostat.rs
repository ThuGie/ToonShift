use serde::Deserialize;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub current_temp: String,
    pub current_setpoint: String,
    pub program_state: String,
    pub burner_info: String,
    pub connection: String,
    pub next_setpoint: String,
    pub next_time: String,
}

impl Status {
    pub fn target(&self) -> Option<i32> {
        self.current_setpoint.parse().ok()
    }
}

#[derive(Clone, Copy)]
pub enum Command {
    Adjust(i32),
    Setpoint(i32),
    Program(bool),
    Schedule,
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

pub struct ScheduleSlot {
    pub day: usize,
    pub start: i64,
    pub end: i64,
    pub preset: String,
}

pub struct Update {
    pub status: Option<Status>,
    pub error: Option<String>,
    pub completed: bool,
    pub schedule: Option<Result<Vec<ScheduleSlot>, String>>,
}

pub fn firmware_get(service: &str, query: &str) -> Result<String, String> {
    fn send(service: &str, query: &str) -> std::io::Result<String> {
        let timeout = Duration::from_secs(if service == "hcb_netcon" { 6 } else { 2 });
        let address = SocketAddr::from(([127, 0, 0, 1], 10080));
        let mut stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        write!(
            stream,
            "GET /{service}?{query} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )?;
        let mut response = String::new();
        stream.take(32768).read_to_string(&mut response)?;
        Ok(response)
    }
    let response = send(service, query).map_err(|err| err.to_string())?;
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

fn schedule() -> Result<Vec<ScheduleSlot>, String> {
    let body = firmware_get("happ_thermstat", "action=getWeeklyList")?;
    let (_, rows) = body
        .split_once("programs: [")
        .ok_or("Unexpected weekly schedule response")?;
    let (rows, _) = rows.rsplit_once(']').ok_or("Incomplete weekly schedule")?;
    rows.split('{')
        .skip(1)
        .map(|row| {
            let (row, _) = row.split_once('}').ok_or("Incomplete schedule entry")?;
            let field = |key| {
                row.split(',')
                    .filter_map(|part| part.split_once(':'))
                    .find(|(name, _)| name.trim() == key)
                    .map(|(_, value)| value.trim().trim_matches('\''))
                    .ok_or_else(|| format!("Missing schedule field {key}"))
            };
            let number = |key| field(key)?.parse::<i64>().map_err(|e| e.to_string());
            let day = number("weekDay")?;
            if !(0..7).contains(&day) {
                return Err("Invalid schedule day".into());
            }
            Ok(ScheduleSlot {
                day: day as usize,
                start: number("startTimeT")?,
                end: number("endTimeT")?,
                preset: field("targetState")?.into(),
            })
        })
        .collect()
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
        Command::Schedule => return Ok(()),
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
            let schedule = matches!(command, Some(Command::Schedule)).then(schedule);
            let heat = command.filter(|command| !matches!(command, Command::Schedule));
            let result = heat.map(apply).transpose();
            let reply = work.take().and_then(|work| work.reply);
            let completed = heat.is_some() && reply.is_none();
            if let Some(reply) = reply {
                let _ = reply.send(result.clone().map(|_| ()));
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
