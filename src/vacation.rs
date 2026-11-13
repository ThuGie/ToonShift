use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Vacation {
    pub id: String,
    pub start: i32,
    pub end: i32,
}

#[derive(Clone, Copy)]
pub struct Id {
    bytes: [u8; 96],
    len: u8,
}

impl Id {
    pub fn parse(value: &str) -> Option<Self> {
        if value.is_empty()
            || value.len() > 96
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_:.{}".contains(&byte))
        {
            return None;
        }
        let mut bytes = [0; 96];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        Some(Self {
            bytes,
            len: value.len() as u8,
        })
    }

    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).expect("validated ASCII")
    }
}

fn parse(xml: &str) -> Result<Vec<Vacation>, String> {
    use crate::boxtalk::field;
    if !xml.contains("GetHolidaysResponse") || xml.contains("<error") || xml.contains("<fault") {
        return Err("Unexpected vacation response".into());
    }
    xml.split("<holiday>")
        .skip(1)
        .map(|entry| {
            let entry = entry
                .split_once("</holiday>")
                .ok_or("Incomplete vacation entry")?
                .0;
            let id = field(entry, "entryId")
                .filter(|id| Id::parse(id).is_some())
                .ok_or("Invalid vacation ID")?
                .to_owned();
            let time = |name| {
                field(entry, name)
                    .and_then(|value| value.parse::<i32>().ok())
                    .filter(|time| *time > 0)
                    .ok_or("Invalid vacation timestamp")
            };
            let (start, end) = (time("startTimeT")?, time("endTimeT")?);
            if end <= start {
                return Err("Invalid vacation interval".into());
            }
            Ok(Vacation { id, start, end })
        })
        .collect()
}

pub fn load() -> Result<Vec<Vacation>, String> {
    let response = crate::boxtalk::thermostat(&["GetHolidays"]).map_err(|e| e.to_string())?;
    let mut entries = parse(&response[0])?;
    entries.sort_by_key(|entry| (entry.start, entry.end));
    Ok(entries)
}

pub fn save(id: Option<Id>, start: i32, end: i32) -> Result<Vec<Vacation>, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    if start <= 0 || end <= start || (start as u64) < now {
        return Err("Enter a future vacation start and a later end".into());
    }
    let existing = load()?;
    if id.is_some_and(|id| existing.iter().all(|entry| entry.id != id.as_str())) {
        return Err("Vacation entry no longer exists".into());
    }
    if existing.iter().any(|entry| {
        id.is_none_or(|id| entry.id != id.as_str()) && start < entry.end && end > entry.start
    }) {
        return Err("Vacation overlaps an existing entry".into());
    }
    let mut args = format!("<startTimeT>{start}</startTimeT><endTimeT>{end}</endTimeT>");
    if let Some(id) = id {
        use std::fmt::Write as _;
        write!(&mut args, "<entryId>{}</entryId>", id.as_str()).expect("writing to String");
    }
    crate::boxtalk::send_thermostat_action("SetHoliday", &args).map_err(|e| e.to_string())?;
    confirm(|entries| {
        entries.iter().any(|entry| {
            id.is_none_or(|id| entry.id == id.as_str()) && entry.start == start && entry.end == end
        })
    })
}

pub fn remove(id: Id) -> Result<Vec<Vacation>, String> {
    let existing = load()?;
    if !existing.iter().any(|entry| entry.id == id.as_str()) {
        return Ok(existing);
    }
    crate::boxtalk::send_thermostat_action(
        "RemoveHoliday",
        &format!("<entryId>{}</entryId>", id.as_str()),
    )
    .map_err(|e| e.to_string())?;
    confirm(|entries| entries.iter().all(|entry| entry.id != id.as_str()))
}

fn confirm(matches: impl Fn(&[Vacation]) -> bool) -> Result<Vec<Vacation>, String> {
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(150));
        if let Ok(entries) = load() {
            if matches(&entries) {
                return Ok(entries);
            }
        }
    }
    Err("Firmware vacation change could not be confirmed".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_entries_and_rejects_truncated_data() {
        let item = "<holiday><entryId>entry-1</entryId><startTimeT>100</startTimeT><endTimeT>200</endTimeT></holiday>";
        let xml = format!("<u:GetHolidaysResponse>{item}{item}</u:GetHolidaysResponse>");
        let vacations = parse(&xml).unwrap();
        assert_eq!(vacations.len(), 2);
        assert_eq!(vacations[0].start, 100);
        assert!(parse("<u:GetHolidaysResponse><holiday>").is_err());
        assert!(Id::parse("x</entryId>").is_none());
    }
}
