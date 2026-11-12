use crate::plugins::{Feed, Place, Plugin, Settings, Source};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Reading {
    pub value: String,
    pub detail: String,
    pub forecast: Vec<ForecastDay>,
}

#[derive(Clone)]
pub struct ForecastDay {
    pub date: String,
    pub temperatures: String,
    pub condition: String,
}

type Key = (String, String, String);
type ResultRow = (Key, Result<Reading, String>);

pub struct Feeds {
    requests: Sender<(Key, Feed, Place)>,
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
        let (requests, jobs) = mpsc::channel::<(Key, Feed, Place)>();
        let (results, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for (key, feed, place) in jobs {
                let value = fetch(&ca, &feed, &place);
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
            matches!(plugin.source, Source::HttpsJson) && settings.installed.contains(&plugin.id)
        };
        let key = |plugin: &Plugin| {
            let place = place(plugin, settings)?;
            Some((
                plugin.id.clone(),
                plugin.release.clone(),
                format!("{},{},{}", place.latitude, place.longitude, place.name),
            ))
        };
        self.due.retain(|existing, _| {
            catalog
                .iter()
                .filter(active)
                .any(|plugin| key(plugin).as_ref() == Some(existing))
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
                        forecast: Vec::new(),
                    }
                }
            };
            values.insert(result_key.0, reading);
            redraw = true;
        }
        for plugin in catalog.iter().filter(active) {
            let Some(key) = key(plugin) else {
                continue;
            };
            if self.due.get(&key).is_none_or(|due| *due <= now) {
                if !self.due.contains_key(&key) {
                    values.remove(&plugin.id);
                }
                self.due.insert(key.clone(), now + Duration::from_secs(900));
                if let (Some(feed), Some(place)) = (&plugin.feed, place(plugin, settings)) {
                    let _ = self.requests.send((key, feed.clone(), place.clone()));
                }
            }
        }
        redraw
    }
}

pub fn fetch(ca: &Path, feed: &Feed, place: &Place) -> Result<Reading, String> {
    let url = feed
        .url
        .replace("{latitude}", &place.latitude.to_string())
        .replace("{longitude}", &place.longitude.to_string());
    if !url.starts_with("https://api.open-meteo.com/v1/") || url.contains(['\r', '\n', '\\']) {
        return Err("Feed URL is outside the supported weather service".into());
    }
    let response = crate::download::get(ca, &url)?;
    let json: serde_json::Value = serde_json::from_slice(&response).map_err(|e| e.to_string())?;
    let number = json
        .pointer(&feed.value)
        .and_then(serde_json::Value::as_f64)
        .ok_or("Feed returned no numeric value")?;
    let detail = json
        .pointer(&feed.condition)
        .and_then(serde_json::Value::as_i64)
        .and_then(|code| feed.labels.get(&code.to_string()))
        .cloned()
        .unwrap_or_default();
    let forecast = feed
        .forecast
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
                    let temperatures = format!(
                        "{:.0}{} / {:.0}{}",
                        low.as_f64()?,
                        feed.suffix,
                        high.as_f64()?,
                        feed.suffix
                    );
                    let condition = code
                        .as_i64()
                        .and_then(|code| feed.labels.get(&code.to_string()))
                        .cloned()
                        .unwrap_or_default();
                    Some(ForecastDay {
                        date,
                        temperatures,
                        condition,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Reading {
        value: format!("{number:.1}{}", feed.suffix),
        detail,
        forecast,
    })
}

pub fn search(
    ca: &Path,
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
    let url = format!("https://geocoding-api.open-meteo.com/v1/search?name={query}&count=5&language={language}&format=json");
    let body = crate::download::get(ca, &url)?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| "Invalid location response")?;
    let Some(results) = json["results"].as_array() else {
        return Ok(Vec::new());
    };
    Ok(results
        .iter()
        .filter_map(|row| {
            let name = row["name"].as_str()?;
            let latitude = row["latitude"].as_f64()?;
            let longitude = row["longitude"].as_f64()?;
            let region = row["admin1"]
                .as_str()
                .or_else(|| row["country"].as_str())
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
        .collect())
}
