use crate::plugins::{Feed, LocationSearch, Place, Plugin, Settings, Source};
use crate::{energy, language::Language};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Reading {
    pub value: String,
    pub detail: String,
    pub rows: Vec<RangeRow>,
}

#[derive(Clone)]
pub struct RangeRow {
    pub date: String,
    pub range: String,
    pub condition: String,
}

type Key = (String, String, String);
type ResultRow = (Key, Result<Reading, String>);

enum Task {
    Feed(Box<Feed>, Place, Language),
    Meter(energy::Meter),
}

pub struct Feeds {
    requests: Sender<(Key, Task)>,
    replies: Receiver<ResultRow>,
    due: HashMap<Key, Instant>,
}

pub fn location(plugin: &Plugin, settings: &Settings) -> usize {
    settings
        .locations
        .get(&plugin.id)
        .and_then(|name| {
            plugin
                .feed
                .as_ref()?
                .places
                .iter()
                .position(|place| &place.name == name)
        })
        .unwrap_or(0)
}

pub fn place<'a>(plugin: &'a Plugin, settings: &'a Settings) -> Option<&'a Place> {
    if let Some(custom) = settings.custom_locations.get(&plugin.id) {
        return Some(custom);
    }
    plugin.feed.as_ref()?.places.get(location(plugin, settings))
}

impl Feeds {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let ca = crate::download::trust(root)?;
        let (requests, jobs) = mpsc::channel::<(Key, Task)>();
        let (results, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for (key, task) in jobs {
                let value = match task {
                    Task::Feed(feed, place, language) => fetch(&ca, &feed, &place, language),
                    Task::Meter(meter) => {
                        energy::load(meter, energy::Period::Day).map(|chart| Reading {
                            value: chart.total.map_or_else(
                                || "No meter data".into(),
                                |value| format!("{value:.2} {}", chart.unit),
                            ),
                            detail: if chart.partial {
                                "Incomplete readings".into()
                            } else {
                                "Recorded total".into()
                            },
                            rows: Vec::new(),
                        })
                    }
                };
                if results.send((key, value)).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            requests,
            replies,
            due: HashMap::new(),
        })
    }

    pub fn tick(
        &mut self,
        catalog: &[Plugin],
        settings: &Settings,
        values: &mut HashMap<String, Reading>,
    ) -> bool {
        let now = Instant::now();
        let mut redraw = false;
        let active = |plugin: &&Plugin| {
            matches!(plugin.source, Source::HttpsJson | Source::MeterArchive)
                && settings.installed.contains(&plugin.id)
        };
        let key = |plugin: &Plugin| {
            let options = place(plugin, settings).map_or_else(String::new, |place| {
                format!(
                    "{},{},{},{}",
                    place.latitude,
                    place.longitude,
                    place.name,
                    settings.language == Language::Dutch
                )
            });
            (
                plugin.id.clone(),
                plugin
                    .installed_release
                    .as_ref()
                    .unwrap_or(&plugin.release)
                    .clone(),
                options,
            )
        };
        self.due.retain(|existing, _| {
            catalog
                .iter()
                .filter(active)
                .any(|plugin| &key(plugin) == existing)
        });
        for (result_key, result) in self.replies.try_iter() {
            if !self.due.contains_key(&result_key) {
                continue;
            }
            let retry = if result.is_ok() { 900 } else { 90 };
            self.due
                .insert(result_key.clone(), now + Duration::from_secs(retry));
            let reading = match result {
                Ok(reading) => reading,
                Err(error) => {
                    eprintln!("{}: {error}", result_key.0);
                    Reading {
                        value: "Unavailable".into(),
                        detail: "Could not refresh data".into(),
                        rows: Vec::new(),
                    }
                }
            };
            values.insert(result_key.0, reading);
            redraw = true;
        }
        for plugin in catalog.iter().filter(active) {
            let key = key(plugin);
            if self.due.get(&key).is_none_or(|due| *due <= now) {
                if !self.due.contains_key(&key) {
                    values.remove(&plugin.id);
                }
                self.due.insert(key.clone(), now + Duration::from_secs(900));
                let task =
                    match plugin.source {
                        Source::HttpsJson => plugin.feed.as_ref().zip(place(plugin, settings)).map(
                            |(feed, place)| {
                                Task::Feed(Box::new(feed.clone()), place.clone(), settings.language)
                            },
                        ),
                        Source::MeterArchive => plugin.meter.map(Task::Meter),
                        _ => None,
                    };
                if let Some(task) = task {
                    let _ = self.requests.send((key, task));
                }
            }
        }
        redraw
    }
}

pub fn fetch(ca: &Path, feed: &Feed, place: &Place, language: Language) -> Result<Reading, String> {
    let url = feed
        .url
        .replace("{latitude}", &place.latitude.to_string())
        .replace("{longitude}", &place.longitude.to_string());
    let response = crate::download::public_json(ca, &url)?;
    let json: serde_json::Value = serde_json::from_slice(&response).map_err(|e| e.to_string())?;
    let number = json
        .pointer(&feed.value)
        .and_then(serde_json::Value::as_f64)
        .ok_or("Feed returned no numeric value")?;
    let label = |code: i64| {
        let key = code.to_string();
        (language == Language::Dutch)
            .then(|| feed.labels_nl.get(&key))
            .flatten()
            .or_else(|| feed.labels.get(&key))
            .cloned()
            .unwrap_or_default()
    };
    let detail = json
        .pointer(&feed.condition)
        .and_then(serde_json::Value::as_i64)
        .map(label)
        .unwrap_or_default();
    let rows = feed
        .series
        .as_ref()
        .map(|forecast| {
            let series = |pointer| json.pointer(pointer).and_then(serde_json::Value::as_array);
            let Some(dates) = series(&forecast.dates) else {
                return Vec::new();
            };
            let (Some(min), Some(max), Some(codes)) = (
                series(&forecast.min),
                series(&forecast.max),
                series(&forecast.codes),
            ) else {
                return Vec::new();
            };
            dates
                .iter()
                .zip(min)
                .zip(max)
                .zip(codes)
                .take(5)
                .filter_map(|(((date, low), high), code)| {
                    let date = date.as_str()?.to_owned();
                    let range = format!(
                        "{:.0}{} / {:.0}{}",
                        low.as_f64()?,
                        feed.suffix,
                        high.as_f64()?,
                        feed.suffix
                    );
                    let condition = code.as_i64().map(label).unwrap_or_default();
                    Some(RangeRow {
                        date,
                        range,
                        condition,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Reading {
        value: format!(
            "{number:.precision$}{}",
            feed.suffix,
            precision = feed.decimals
        ),
        detail,
        rows,
    })
}

pub fn search(
    ca: &Path,
    search: &LocationSearch,
    query: &str,
    language: crate::language::Language,
) -> Result<Vec<Place>, String> {
    let query = query.trim();
    if query.len() < 2 || query.len() > 100 {
        return Err("Enter at least two letters.".into());
    }
    let language = if language == crate::language::Language::Dutch {
        "nl"
    } else {
        "en"
    };
    let query = crate::download::query_value(query);
    let url = search
        .url
        .replace("{query}", &query)
        .replace("{language}", language);
    let body = crate::download::public_json(ca, &url)?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| "Invalid location response")?;
    let Some(results) = json
        .pointer(&search.results)
        .and_then(serde_json::Value::as_array)
    else {
        return Ok(Vec::new());
    };
    Ok(results
        .iter()
        .filter_map(|row| {
            let name = row.pointer(&search.name)?.as_str()?;
            let latitude = row.pointer(&search.latitude)?.as_f64()?;
            let longitude = row.pointer(&search.longitude)?.as_f64()?;
            let region = search
                .region
                .iter()
                .find_map(|path| row.pointer(path)?.as_str())
                .unwrap_or("");
            let name = if region.is_empty() {
                name.to_owned()
            } else {
                format!("{name}, {region}")
            };
            ((-90.0..=90.0).contains(&latitude) && (-180.0..=180.0).contains(&longitude)).then_some(
                Place {
                    name,
                    latitude,
                    longitude,
                },
            )
        })
        .take(5)
        .collect())
}
