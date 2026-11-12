use serde_json::{json, Value};
use std::fmt::Write;

pub const DAY: u16 = 24 * 60;
const WEEK: u16 = 7 * DAY;
const OBJECT: &str = "package=happ_thermstat&internalAddress=thermostatProgram";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub start: u16,
    pub end: u16,
    pub preset: u8,
}

impl Slot {
    pub fn day(self) -> usize {
        usize::from(self.start / DAY)
    }
    pub fn label(self, end: bool) -> String {
        let minute = if end { self.end } else { self.start };
        let day = usize::from(minute / DAY);
        let time = format!("{:02}:{:02}", (minute % DAY) / 60, minute % 60);
        if day == self.day() {
            time
        } else {
            format!(
                "{} {time}",
                ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][day]
            )
        }
    }
    pub fn preset_name(self) -> &'static str {
        ["Comfort", "Home", "Sleep", "Away"]
            .get(usize::from(self.preset))
            .copied()
            .unwrap_or("Unknown")
    }
    pub fn shift(&mut self, end: bool, offset: i16) {
        let field = if end { &mut self.end } else { &mut self.start };
        *field = (*field as i16 + offset).rem_euclid(WEEK as i16) as u16;
    }
    fn duration(self) -> u16 {
        (self.end + WEEK - self.start) % WEEK
    }
}

pub fn open_period(day: usize, slots: &[Slot]) -> Option<Slot> {
    let first = day as u16 * DAY;
    (first..first + DAY).step_by(15).find_map(|start| {
        let end = (start + 15) % WEEK;
        let free = slots.iter().all(|slot| {
            let from_existing = (start + WEEK - slot.start) % WEEK;
            let from_new = (slot.start + WEEK - start) % WEEK;
            from_existing >= slot.duration() && from_new >= 15
        });
        free.then_some(Slot {
            start,
            end,
            preset: 1,
        })
    })
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Replace { original: Slot, new: Slot },
    Split { slot: Slot },
    Remove { slot: Slot },
    Add { slot: Slot },
    CopyDay { source: u8, days: u8 },
}

type Rows = Vec<(Slot, Value)>;

pub fn revision(slots: &[Slot]) -> u64 {
    let mut slots = slots.to_vec();
    slots.sort_by_key(|slot| (slot.start, slot.end, slot.preset));
    slots
        .into_iter()
        .flat_map(|slot| [slot.start, slot.end, u16::from(slot.preset)])
        .fold(0xcbf29ce484222325u64, |hash, value| {
            (hash ^ u64::from(value)).wrapping_mul(0x100000001b3)
        })
}

fn copy_day(rows: &mut Rows, source: u8, days: u8) -> Result<(), String> {
    if source >= 7 || days == 0 || days & !0x7f != 0 || days & (1 << source) != 0 {
        return Err("Choose a source day and different destination days.".into());
    }
    let mut by_day: [Rows; 7] = Default::default();
    for (slot, entry) in rows.iter() {
        let mut start = slot.start;
        let mut remaining = slot.duration();
        while remaining > 0 {
            let length = remaining.min(DAY - start % DAY);
            let end = (start + length) % WEEK;
            by_day[usize::from(start / DAY)].push((
                Slot {
                    start,
                    end,
                    preset: slot.preset,
                },
                entry.clone(),
            ));
            start = end;
            remaining -= length;
        }
    }
    let template = by_day[usize::from(source)].clone();
    if template.is_empty() {
        return Err("The source day has no periods.".into());
    }
    for (day, periods) in by_day.iter_mut().enumerate() {
        if days & (1 << day) == 0 {
            continue;
        }
        let offset = day as u16 * DAY;
        *periods = template
            .iter()
            .map(|(slot, entry)| {
                let start = offset + slot.start % DAY;
                (
                    Slot {
                        start,
                        end: (start + slot.duration()) % WEEK,
                        preset: slot.preset,
                    },
                    entry.clone(),
                )
            })
            .collect();
    }
    let mut merged: Rows = Vec::new();
    for periods in &mut by_day {
        periods.sort_by_key(|(slot, _)| slot.start);
    }
    for row in by_day.into_iter().flatten() {
        if let Some((last, fields)) = merged.last_mut() {
            if last.end == row.0.start
                && last.preset == row.0.preset
                && *fields == row.1
                && last.duration() + row.0.duration() < WEEK
            {
                last.end = row.0.end;
                continue;
            }
        }
        merged.push(row);
    }
    *rows = merged;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_midnight_intersections_and_keeps_non_destination_days() {
        let marker = json!({"entry":[{"type":"weekly_recurring"}]});
        let original = Slot {
            start: WEEK - 60,
            end: 90,
            preset: 2,
        };
        let mut rows = vec![(original, marker)];
        copy_day(&mut rows, 0, 1 << 1).unwrap();
        let actual: Vec<_> = rows.into_iter().map(|(slot, _)| slot).collect();
        assert_eq!(
            actual,
            vec![
                Slot {
                    start: 0,
                    end: 90,
                    preset: 2
                },
                Slot {
                    start: DAY,
                    end: DAY + 90,
                    preset: 2
                },
                Slot {
                    start: WEEK - 60,
                    end: 0,
                    preset: 2
                },
            ]
        );
    }

    #[test]
    fn revisions_ignore_order_and_detect_changes() {
        let a = Slot {
            start: 20,
            end: 30,
            preset: 2,
        };
        let b = Slot {
            start: 30,
            end: 40,
            preset: 1,
        };
        assert_eq!(revision(&[a, b]), revision(&[b, a]));
        assert_ne!(revision(&[a, b]), revision(&[a, Slot { preset: 3, ..b }]));
        assert!(!valid_slot(Slot {
            start: u16::MAX,
            ..a
        }));
    }
}

struct Program {
    slots: Vec<Slot>,
    entries: Vec<Value>,
    uuid: String,
    visibility: String,
}

fn read() -> Result<Program, String> {
    let body =
        crate::thermostat::firmware_get("hcb_config", &format!("action=getObjectConfig&{OBJECT}"))?;
    let json: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let device = json["device"]
        .as_array()
        .ok_or("Firmware returned no program device")?;
    let field = |name: &str| {
        device
            .iter()
            .find_map(|field| field[name].as_str())
            .ok_or("Missing program metadata")
    };
    let uuid = field("uuid")?;
    let visibility = field("visibility")?;
    if !uuid.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-') || !matches!(visibility, "0" | "1")
    {
        return Err("Invalid program metadata".into());
    }
    let entries = device
        .iter()
        .find_map(|item| item["schedule"].as_array())
        .ok_or("Firmware returned no weekly schedule")?;
    let slots = entries
        .iter()
        .map(|row| {
            let fields = row["entry"].as_array().ok_or("Malformed schedule entry")?;
            let get = |name: &str| {
                fields
                    .iter()
                    .find_map(|field| field[name].as_str())
                    .ok_or_else(|| format!("Missing {name}"))
            };
            let number = |name| get(name)?.parse::<u16>().map_err(|e| e.to_string());
            let time = |day, hour, min| {
                let (d, h, m) = (number(day)?, number(hour)?, number(min)?);
                if d >= 7 || h >= 24 || m >= 60 {
                    return Err("Invalid schedule time".to_owned());
                }
                Ok(d * DAY + h * 60 + m)
            };
            if get("type")? != "weekly_recurring" {
                return Err("Unsupported schedule entry type".into());
            }
            Ok(Slot {
                start: time("startDayOfWeek", "startHour", "startMin")?,
                end: time("endDayOfWeek", "endHour", "endMin")?,
                preset: number("targetState")?
                    .try_into()
                    .map_err(|_| "Invalid preset")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Program {
        slots,
        entries: entries.clone(),
        uuid: uuid.into(),
        visibility: visibility.into(),
    })
}

pub fn load() -> Result<Vec<Slot>, String> {
    let mut slots = read()?.slots;
    slots.sort_by_key(|slot| slot.start);
    Ok(slots)
}

pub fn edit(edit: Edit, expected_revision: Option<u64>) -> Result<Vec<Slot>, String> {
    let Program {
        slots,
        entries,
        uuid,
        visibility,
    } = read()?;
    if expected_revision.is_some_and(|expected| expected != revision(&slots)) {
        return Err("Schedule revision conflict".into());
    }
    let mut rows: Vec<_> = slots.into_iter().zip(entries).collect();
    let original = match edit {
        Edit::Replace { original, .. }
        | Edit::Split { slot: original }
        | Edit::Remove { slot: original } => Some(original),
        Edit::Add { .. } | Edit::CopyDay { .. } => None,
    };
    let find = || {
        rows.iter()
            .position(|(slot, _)| Some(*slot) == original)
            .ok_or("The schedule changed. Reopen it and try again.")
    };
    match edit {
        Edit::Add { slot } => {
            if rows.len() >= 48 {
                return Err("The program already has 48 periods.".into());
            }
            if !valid_slot(slot) {
                return Err("Choose a valid preset and period.".into());
            }
            rows.push((slot, json!({"entry":[{"type":"weekly_recurring"}]})));
        }
        Edit::Replace { original, new } => {
            let index = find()?;
            if !valid_slot(new) {
                return Err(
                    "Choose a valid preset and a period between one minute and 48 hours.".into(),
                );
            }
            let mut neighbors = rows.iter_mut().enumerate().filter(|(i, _)| *i != index);
            for (_, (neighbor, _)) in &mut neighbors {
                if neighbor.end == original.start {
                    neighbor.end = new.start;
                }
                if neighbor.start == original.end {
                    neighbor.start = new.end;
                }
            }
            rows[index].0 = new;
        }
        Edit::Split { slot: original } => {
            let index = find()?;
            if original.duration() < 2 || rows.len() >= 48 {
                return Err("This period cannot be split.".into());
            }
            let middle = (original.start + original.duration() / 2) % WEEK;
            rows[index].0.end = middle;
            rows.push((
                Slot {
                    start: middle,
                    ..original
                },
                rows[index].1.clone(),
            ));
        }
        Edit::Remove { slot: original } => {
            let index = find()?;
            if rows.len() <= 1 {
                return Err("Keep at least one schedule period.".into());
            }
            rows.remove(index);
            if let Some((previous, _)) =
                rows.iter_mut().find(|(slot, _)| slot.end == original.start)
            {
                previous.end = original.end;
            }
        }
        Edit::CopyDay { source, days } => copy_day(&mut rows, source, days)?,
    }
    rows.sort_by_key(|(slot, _)| slot.start);
    if rows.len() > 48 {
        return Err("The program would exceed 48 periods.".into());
    }
    for ((slot, _), (next, _)) in rows.iter().zip(rows.iter().cycle().skip(1)) {
        let gap = (next.start + WEEK - slot.start) % WEEK;
        if slot.duration() == 0 || (rows.len() > 1 && (gap == 0 || slot.duration() > gap)) {
            return Err("The change would create overlapping or empty periods.".into());
        }
    }
    let expected: Vec<_> = rows.iter().map(|(slot, _)| *slot).collect();
    let mut payload = String::from("<Config><device><schedule>");
    for (slot, row) in rows {
        let fields = row["entry"].as_array().expect("validated firmware entry");
        payload.push_str("<entry type=\"weekly_recurring\"><type>weekly_recurring</type>");
        let values = [
            ("startDayOfWeek", slot.start / DAY),
            ("startHour", (slot.start % DAY) / 60),
            ("startMin", slot.start % 60),
            ("endDayOfWeek", slot.end / DAY),
            ("endHour", (slot.end % DAY) / 60),
            ("endMin", slot.end % 60),
            ("targetState", u16::from(slot.preset)),
        ];
        for (name, value) in values {
            write!(payload, "<{name}>{value}</{name}>").unwrap();
        }
        for (name, value) in fields.iter().filter_map(Value::as_object).flatten() {
            if name == "type" || values.iter().any(|(field, _)| field == name) {
                continue;
            }
            if !name.starts_with(|c: char| c.is_ascii_alphabetic())
                || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return Err("Unsupported firmware schedule field".into());
            }
            let value = value
                .as_str()
                .ok_or("Unsupported nested schedule field")?
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            write!(payload, "<{name}>{value}</{name}>").unwrap();
        }
        payload.push_str("</entry>");
    }
    write!(payload, "</schedule><package>happ_thermstat</package><uuid>{uuid}</uuid><type>program</type><internalAddress>thermostatProgram</internalAddress><visibility>{visibility}</visibility></device></Config>").unwrap();
    store_config(&payload)?;
    for delay in [100, 150, 300] {
        std::thread::sleep(std::time::Duration::from_millis(delay));
        if let Ok(actual) = load() {
            if actual == expected {
                return Ok(actual);
            }
        }
    }
    Err("The thermostat did not confirm the requested schedule. Reopen Schedule to refresh.".into())
}

fn valid_slot(slot: Slot) -> bool {
    slot.start < WEEK
        && slot.end < WEEK
        && slot.preset <= 3
        && (1..=2 * DAY).contains(&slot.duration())
}

pub fn store_config(payload: &str) -> Result<(), String> {
    let mut encoded = String::with_capacity(payload.len() * 2);
    for b in payload.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            encoded.push(char::from(b));
        } else {
            write!(encoded, "%{b:02X}").unwrap();
        }
    }
    let response = crate::thermostat::firmware_get(
        "hcb_config",
        &format!("action=setObjectConfig&packageName=happ_thermstat&config={encoded}"),
    )?;
    if serde_json::from_str::<Value>(&response)
        .ok()
        .is_none_or(|response| response["result"] != "ok")
    {
        return Err("The firmware rejected the thermostat configuration.".into());
    }
    Ok(())
}
