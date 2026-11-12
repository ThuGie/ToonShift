use crate::{boxtalk, download, thermostat};
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Boiler,
    Meters,
    Devices,
}

#[derive(Clone, Copy)]
pub struct HotWater {
    pub enabled: bool,
    pub target: i32,
    pub min: i32,
    pub max: i32,
}

pub struct Boiler {
    pub open_therm: bool,
    pub connected: bool,
    pub hot_water: Option<HotWater>,
}

#[derive(Clone)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub connected: Option<bool>,
    pub meter: bool,
    pub switch: bool,
}

pub struct Devices {
    pub nodes: Vec<Node>,
    pub p1_sources: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PairMode {
    Include,
    Exclude,
}

pub enum PairStep {
    Finished(&'static str),
    Keys(Vec<String>, bool),
    Pin { digits: usize, dsk: String },
    ControllerPin(String),
}

pub enum PairRequest {
    Start,
    Stop,
    Grant { keys: Vec<String>, csa: bool },
    Pin(String),
    ConfirmController,
}

#[derive(Clone, Copy)]
pub enum BoilerChange {
    Protocol(bool),
    HotWater(HotWater),
}

pub enum Job {
    Refresh,
    Boiler(BoilerChange),
    Pair(PairMode, PairRequest),
    Rename(Node, String),
    Switch(Node, bool),
}

enum Reply {
    Snapshot(Result<Boiler, String>, Result<Devices, String>),
    Boiler(Result<Boiler, String>),
    Pair(Result<PairStep, String>),
    Device(Result<(), String>, Result<Devices, String>),
}

pub struct State {
    pub boiler: Option<Result<Boiler, String>>,
    pub devices: Option<Result<Devices, String>>,
    pub pairing: Option<PairMode>,
    pub step: Option<PairStep>,
    pub busy: bool,
    pub message: Option<String>,
    generation: u64,
    sender: std::sync::mpsc::Sender<(u64, Reply)>,
    receiver: std::sync::mpsc::Receiver<(u64, Reply)>,
}

impl State {
    pub fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            boiler: None,
            devices: None,
            pairing: None,
            step: None,
            busy: false,
            message: None,
            generation: 0,
            sender,
            receiver,
        }
    }

    pub fn submit(&mut self, job: Job) {
        let stop = matches!(job, Job::Pair(_, PairRequest::Stop));
        if (self.busy && !stop) || (self.pairing.is_some() && !matches!(job, Job::Pair(_, _))) {
            return;
        }
        if let Job::Pair(mode, _) = &job {
            self.pairing = Some(*mode);
            self.step = None;
        }
        self.busy = true;
        self.message = None;
        self.generation += 1;
        let generation = self.generation;
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let response = match job {
                Job::Refresh => Reply::Snapshot(boiler(), devices()),
                Job::Boiler(change) => Reply::Boiler(configure(change)),
                Job::Pair(mode, request) => Reply::Pair(pair(mode, request)),
                Job::Rename(node, name) => Reply::Device(rename(node, name), devices()),
                Job::Switch(node, on) => Reply::Device(switch(node, on), devices()),
            };
            let _ = sender.send((generation, response));
        });
    }

    pub fn update(&mut self) -> bool {
        let mut redraw = false;
        while let Ok((generation, response)) = self.receiver.try_recv() {
            if generation != self.generation {
                continue;
            }
            self.busy = false;
            redraw = true;
            match response {
                Reply::Snapshot(boiler, devices) => {
                    self.boiler = Some(boiler);
                    self.devices = Some(devices);
                }
                Reply::Boiler(Ok(boiler)) => {
                    self.boiler = Some(Ok(boiler));
                    self.message = Some("Boiler settings saved".into());
                }
                Reply::Boiler(Err(error)) => self.message = Some(error),
                Reply::Pair(Ok(PairStep::Finished(message))) => {
                    self.pairing = None;
                    self.submit(Job::Refresh);
                    self.message = Some(message.into());
                }
                Reply::Pair(Ok(step)) => self.step = Some(step),
                Reply::Pair(Err(error)) => {
                    self.pairing = None;
                    self.message = Some(error);
                }
                Reply::Device(result, inventory) => {
                    self.devices = Some(inventory);
                    self.message = Some(
                        result
                            .err()
                            .unwrap_or_else(|| "Device command accepted".into()),
                    );
                }
            }
        }
        redraw
    }
}

pub fn boiler() -> Result<Boiler, String> {
    let status = thermostat::status()?;
    let replies =
        boxtalk::thermostat(&["GetBoilerType", "GetDhwSettings"]).map_err(|err| err.to_string())?;
    let open_therm = match boxtalk::field(&replies[0], "ot") {
        Some("0") => false,
        Some("1") => true,
        _ => return Err("Boiler mode unavailable".into()),
    };
    let hot_water = (|| {
        let read = |name| boxtalk::field(&replies[1], name)?.parse::<i32>().ok();
        let min = read("dhwMinSetpoint")?;
        let max = read("dhwMaxSetpoint")?;
        let target = read("dhwSetpoint")?;
        let enabled = read("dhwEnabled")? == 1;
        (min > 0 && max <= 100 && (min..=max).contains(&target)).then_some(HotWater {
            enabled,
            target,
            min,
            max,
        })
    })();
    Ok(Boiler {
        open_therm,
        connected: status.connection != "0",
        hot_water,
    })
}

pub fn configure(change: BoilerChange) -> Result<Boiler, String> {
    let before = boiler()?;
    let (verb, args) = match change {
        BoilerChange::Protocol(mode) if mode == before.open_therm => return Ok(before),
        BoilerChange::Protocol(mode) => ("SetBoilerType", format!("<ot>{}</ot>", u8::from(mode))),
        BoilerChange::HotWater(request) => {
            let limits = before
                .hot_water
                .filter(|_| before.connected && before.open_therm)
                .ok_or("Hot-water settings require a connected OpenTherm boiler.")?;
            if !(limits.min..=limits.max).contains(&request.target) {
                return Err("Hot-water temperature is outside the firmware limits.".into());
            }
            (
                "SetDhwSettings",
                format!(
                    "<dhwEnabled>{}</dhwEnabled><dhwSetpoint>{}</dhwSetpoint>",
                    u8::from(request.enabled),
                    request.target
                ),
            )
        }
    };
    let replies = boxtalk::thermostat_actions(&[(verb, &args)]).map_err(|err| err.to_string())?;
    if boxtalk::field(&replies[0], "Error").is_some()
        || boxtalk::field(&replies[0], "result") == Some("error")
    {
        return Err("Firmware rejected the boiler setting.".into());
    }
    let after = boiler()?;
    let matched = match change {
        BoilerChange::Protocol(mode) => mode == after.open_therm,
        BoilerChange::HotWater(request) => after.hot_water.is_some_and(|actual| {
            actual.enabled == request.enabled && actual.target == request.target
        }),
    };
    if matched {
        Ok(after)
    } else {
        Err("The firmware did not confirm the change.".into())
    }
}

fn zwave(query: &str, seconds: u64) -> Result<Value, String> {
    let body = thermostat::firmware_get_timeout("hdrv_zwave", query, Duration::from_secs(seconds))?;
    let value = zwave_response(&body)?;
    if value["result"] == "error"
        || value["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    {
        return Err(value["error"]
            .as_str()
            .unwrap_or("Z-Wave operation failed")
            .to_owned());
    }
    Ok(value)
}

fn zwave_response(body: &str) -> Result<Value, String> {
    let mut messages = serde_json::Deserializer::from_str(body).into_iter::<Value>();
    let mut latest = messages
        .next()
        .ok_or("Empty Z-Wave response")?
        .map_err(|_| "Invalid Z-Wave response")?;
    for message in messages {
        latest = message.map_err(|_| "Invalid Z-Wave response")?;
    }
    Ok(latest)
}

pub fn devices() -> Result<Devices, String> {
    let inventory = zwave("action=getDevices.json", 3)?;
    let entries = inventory.as_object().ok_or("Invalid Z-Wave inventory")?;
    let mut nodes = Vec::new();
    for (key, entry) in entries {
        if !key.starts_with("dev_") || key.contains('.') {
            continue;
        }
        let kind = entry["type"].as_str().unwrap_or("Z-Wave");
        let name = entry["DeviceName"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .or_else(|| entry["name"].as_str())
            .unwrap_or(kind);
        let classes = entry["supportedCC"].as_str().unwrap_or_default();
        let connected = match entry["IsConnected"].as_str() {
            Some("1") => Some(true),
            Some("0") => Some(false),
            _ => None,
        };
        if let Some(uuid) = entry["uuid"].as_str() {
            nodes.push(Node {
                id: uuid.into(),
                name: name.into(),
                kind: kind.into(),
                connected,
                meter: kind.contains("METER"),
                switch: classes
                    .split_whitespace()
                    .any(|v| v.eq_ignore_ascii_case("25")),
            });
        }
    }
    let p1_sources = thermostat::firmware_get("hcb_config", "action=getDeviceList.json")
        .ok()
        .and_then(|body| serde_json::from_str::<Value>(&body).ok())
        .and_then(|value| {
            value["device"].as_object().map(|devices| {
                devices
                    .values()
                    .filter(|entry| {
                        entry["package"] == "hdrv_p1" && entry["type"] == "connection_device"
                    })
                    .count()
            })
        })
        .unwrap_or(0);
    Ok(Devices { nodes, p1_sources })
}

pub fn rename(node: Node, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
        return Err("Enter a name between 1 and 64 bytes.".into());
    }
    zwave(
        &format!(
            "action=SetDeviceName&uuid={}&deviceName={}",
            download::query_value(&node.id),
            download::query_value(name)
        ),
        3,
    )?;
    if devices()?
        .nodes
        .iter()
        .any(|entry| entry.id == node.id && entry.name == name)
    {
        Ok(())
    } else {
        Err("The new name could not be confirmed.".into())
    }
}

pub fn switch(node: Node, enabled: bool) -> Result<(), String> {
    if !node.switch {
        return Err("This device does not support binary switching.".into());
    }
    zwave(
        &format!(
            "action=basicCommand&uuid={}&state={}",
            download::query_value(&node.id),
            if enabled { 255 } else { 0 }
        ),
        3,
    )?;
    Ok(())
}

pub fn pair(mode: PairMode, request: PairRequest) -> Result<PairStep, String> {
    let action = if mode == PairMode::Include {
        "IncludeDevice"
    } else {
        "ExcludeDevice"
    };
    let (query, timeout) = match request {
        PairRequest::Start if mode == PairMode::Include => ("event=add&lowPower=0".into(), 60),
        PairRequest::Start => ("event=delete".into(), 30),
        PairRequest::Stop => ("event=stop".into(), 3),
        PairRequest::Grant { keys, csa } => {
            if keys.is_empty()
                || keys.iter().any(|key| {
                    key.is_empty()
                        || !key
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_ ".contains(&b))
                })
            {
                return Err("Unrecognized Z-Wave security key.".into());
            }
            let keys: String = keys.iter().map(|key| format!("<key>{key}</key>")).collect();
            (
                format!(
                    "event=keyGrant&accept=1&csa={}&keys={}",
                    u8::from(csa),
                    download::query_value(&keys)
                ),
                30,
            )
        }
        PairRequest::Pin(pin) if pin.len() <= 10 && pin.bytes().all(|b| b.is_ascii_digit()) => {
            (format!("event=ssaAuth&accept=1&dskUserInput={pin}"), 120)
        }
        PairRequest::Pin(_) => return Err("Invalid Z-Wave PIN.".into()),
        PairRequest::ConfirmController => ("event=csaAuth".into(), 120),
    };
    let stop = query == "event=stop";
    let response = zwave(
        &format!("action={action}&timeout={timeout}&{query}"),
        timeout + 4,
    )?;
    let step = response["status"].as_str().unwrap_or_default();
    Ok(match step {
        "added" => PairStep::Finished("Device added"),
        "deleted" => PairStep::Finished("Device removed"),
        "canceled" | "stopped" => PairStep::Finished("Setup cancelled"),
        "keyGrant" => PairStep::Keys(
            response["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .take(6)
                .collect(),
            response["csa"].as_str() == Some("1"),
        ),
        "ssaAuth" => {
            let digits = response["userInputDigits"]
                .as_u64()
                .or_else(|| response["userInputDigits"].as_str()?.parse().ok())
                .ok_or("Invalid Z-Wave PIN request")?;
            if digits > 10 {
                return Err("Invalid Z-Wave PIN request".into());
            }
            PairStep::Pin {
                digits: digits as usize,
                dsk: response["dsk"].as_str().unwrap_or_default().into(),
            }
        }
        "csaAuth" => PairStep::ControllerPin(response["dsk"].as_str().unwrap_or_default().into()),
        _ if stop => PairStep::Finished("Setup cancelled"),
        "not_primary" => return Err("The controller is not primary.".into()),
        "timeout" => return Err("Z-Wave setup timed out.".into()),
        _ => return Err("Z-Wave setup was not confirmed.".into()),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_the_final_status_in_the_firmware_stop_response() {
        let response =
            super::zwave_response(r#"{"status":"canceled"}{"status":"stopped"}"#).unwrap();
        assert_eq!(response["status"], "stopped");
        assert!(super::zwave_response(r#"{"status":"canceled"}garbage"#).is_err());
    }
}
