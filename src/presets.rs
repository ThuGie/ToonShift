use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;

pub const NAMES: [&str; 5] = ["Comfort", "Home", "Sleep", "Away", "Vacation"];

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct Preset {
    pub id: u8,
    pub temperature: i32,
    pub hot_water: bool,
}

const OBJECT: &str =
    "action=getObjectConfig&package=happ_thermstat&internalAddress=thermostatStates";

type States = BTreeMap<u8, BTreeMap<String, String>>;

fn read() -> Result<(String, States), String> {
    let body = crate::thermostat::firmware_get("hcb_config", OBJECT)?;
    let json: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let device = json["device"]
        .as_array()
        .ok_or("No preset configuration returned")?;
    let uuid = device
        .iter()
        .find_map(|item| item["uuid"].as_str())
        .ok_or("Preset configuration UUID missing")?;
    if uuid.len() != 36 || !uuid.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-') {
        return Err("Invalid preset configuration UUID".into());
    }
    let states = device
        .iter()
        .find_map(|item| item["states"].as_array())
        .ok_or("No preset states returned")?;
    let rows = states
        .iter()
        .map(|state| {
            let fields = state["state"].as_array().ok_or("Malformed preset state")?;
            let fields = fields
                .iter()
                .filter_map(Value::as_object)
                .flatten()
                .map(|(name, value)| {
                    Ok((
                        name.clone(),
                        value.as_str().ok_or("Unexpected preset field")?.to_owned(),
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, String>>()?;
            let id = fields
                .get("id")
                .and_then(|id| id.parse::<u8>().ok())
                .ok_or("Invalid preset ID")?;
            Ok((id, fields))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    Ok((uuid.to_owned(), rows))
}

pub fn load() -> Result<Vec<Preset>, String> {
    let (_, rows) = read()?;
    rows.iter()
        .filter(|(id, _)| usize::from(**id) < NAMES.len())
        .map(|(&id, fields)| {
            let temperature = fields
                .get("tempValue")
                .and_then(|value| value.parse::<i32>().ok())
                .ok_or("Invalid preset temperature")?;
            let hot_water = fields.get("dhw").is_some_and(|value| value == "1");
            Ok(Preset {
                id,
                temperature,
                hot_water,
            })
        })
        .collect()
}

pub fn save(preset: Preset) -> Result<Vec<Preset>, String> {
    if !(600..=3000).contains(&preset.temperature) || usize::from(preset.id) >= NAMES.len() {
        return Err("Preset temperature must be between 6°C and 30°C.".into());
    }
    let (uuid, mut states) = read()?;
    let fields = states.get_mut(&preset.id).ok_or("Preset not found")?;
    fields.insert("tempValue".into(), preset.temperature.to_string());
    fields.insert("dhw".into(), u8::from(preset.hot_water).to_string());
    let mut xml = String::from("<Config><device><states>");
    for state in states.values() {
        xml.push_str("<state>");
        for (name, value) in state {
            if !name.starts_with(|c: char| c.is_ascii_alphabetic())
                || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return Err("Unsupported firmware preset field".into());
            }
            let value = value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            write!(xml, "<{name}>{value}</{name}>").unwrap();
        }
        xml.push_str("</state>");
    }
    write!(xml, "</states><package>happ_thermstat</package><uuid>{uuid}</uuid><type>states</type><name>thermostatStates</name><internalAddress>thermostatStates</internalAddress><visibility>0</visibility><statesSaved>1</statesSaved></device></Config>").unwrap();
    crate::schedule::store_config(&xml)?;
    for delay in [100, 150, 300] {
        std::thread::sleep(std::time::Duration::from_millis(delay));
        if let Ok(actual) = load() {
            if actual.iter().any(|row| {
                row.id == preset.id
                    && row.temperature == preset.temperature
                    && row.hot_water == preset.hot_water
            }) {
                return Ok(actual);
            }
        }
    }
    Err("The thermostat did not confirm the preset change.".into())
}
