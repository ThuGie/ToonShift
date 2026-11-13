use crate::plugins::{Feed, LocationSearch, Place, Plugin, Settings, Source};
use crate::{energy, language::Language};
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

#[derive(Clone, serde::Deserialize)]
pub struct Reading {
    pub value: String,
    pub detail: String,
    pub rows: Vec<RangeRow>,
}

#[derive(Clone, serde::Deserialize)]
pub struct RangeRow {
    pub date: String,
    pub range: String,
    pub condition: String,
}

type Key = (String, String, String);
type ResultRow = (Key, Result<Reading, String>);

enum Task {
    Feed(Box<Feed>, Place, Language, Option<String>),
    Meter(energy::Meter),
    Wasm(String, Vec<u8>),
}

pub struct Feeds {
    requests: mpsc::SyncSender<(Key, Task)>,
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
        let (requests, jobs) = mpsc::sync_channel::<(Key, Task)>(32);
        let (results, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for (key, task) in jobs {
                let value = match task {
                    Task::Wasm(code, input) => crate::plugin_runtime::decode(&code)
                        .and_then(|code| crate::plugin_runtime::render(&code, &input))
                        .map(|value| Reading {
                            value,
                            detail: String::new(),
                            rows: Vec::new(),
                        }),
                    Task::Feed(feed, place, language, code) => {
                        fetch(&ca, &feed, &place, language, code.as_deref())
                    }
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
        telemetry: &crate::telemetry::Readings,
        values: &mut HashMap<String, Reading>,
    ) -> bool {
        let now = Instant::now();
        let mut redraw = false;
        let active = |plugin: &&Plugin| {
            matches!(
                plugin.source,
                Source::HttpsJson | Source::MeterArchive | Source::Wasm
            ) && settings.installed.contains(&plugin.id)
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
            let retry = if result.is_ok() {
                if catalog.iter().any(|plugin| {
                    plugin.id == result_key.0 && matches!(plugin.source, Source::Wasm)
                }) {
                    15
                } else {
                    900
                }
            } else {
                90
            };
            self.due
                .insert(result_key.clone(), now + Duration::from_secs(retry));
            let reading = match result {
                Ok(reading) => reading,
                Err(error) => {
                    eprintln!("{}: {error}", result_key.0);
                    if let Some(reading) = values.get_mut(&result_key.0) {
                        reading.detail = "Could not refresh data".into();
                        redraw = true;
                        continue;
                    }
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
                                Task::Feed(
                                    Box::new(feed.clone()),
                                    place.clone(),
                                    settings.language,
                                    plugin.wasm.clone(),
                                )
                            },
                        ),
                        Source::MeterArchive => plugin.meter.map(Task::Meter),
                        Source::Wasm => plugin.wasm.clone().map(|code| {
                            let input = plugin
                                .metric
                                .map(|metric| crate::telemetry::display(metric, telemetry))
                                .unwrap_or_default();
                            Task::Wasm(code, input.into_bytes())
                        }),
                        _ => None,
                    };
                if let Some(task) = task {
                    if self.requests.try_send((key.clone(), task)).is_err() {
                        self.due.insert(key, now + Duration::from_secs(5));
                    }
                }
            }
        }
        redraw
    }
}

pub fn fetch(
    ca: &Path,
    feed: &Feed,
    place: &Place,
    language: Language,
    code: Option<&str>,
) -> Result<Reading, String> {
    let url = feed
        .url
        .replace("{latitude}", &place.latitude.to_string())
        .replace("{longitude}", &place.longitude.to_string());
    let response = crate::download::public_json(ca, &url)?;
    if let Some(code) = code {
        let mut reading: Reading = parse_executable_response(code, &response, language, false)?;
        reading.rows.truncate(5);
        return Ok(reading);
    }
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

fn parse_executable_response<T: serde::de::DeserializeOwned>(
    code: &str,
    response: &[u8],
    language: Language,
    search: bool,
) -> Result<T, String> {
    let mut input = Vec::with_capacity(response.len() + 2);
    input.extend([u8::from(search), u8::from(language == Language::Dutch)]);
    input.extend_from_slice(response);
    let code = crate::plugin_runtime::decode(code)?;
    let output = crate::plugin_runtime::render_feed(&code, &input)?;
    serde_json::from_str(&output).map_err(|_| "Plugin returned an invalid response".into())
}

pub fn search(
    ca: &Path,
    search: &LocationSearch,
    query: &str,
    language: crate::language::Language,
    code: Option<&str>,
) -> Result<Vec<Place>, String> {
    let query = query.trim();
    if query.len() < 2 || query.len() > 100 {
        return Err("Enter at least two letters.".into());
    }
    let locale = if language == crate::language::Language::Dutch {
        "nl"
    } else {
        "en"
    };
    let query = crate::download::query_value(query);
    let url = search
        .url
        .replace("{query}", &query)
        .replace("{language}", locale);
    let body = crate::download::public_json(ca, &url)?;
    if let Some(code) = code {
        let mut places: Vec<Place> = parse_executable_response(code, &body, language, true)?;
        places.retain(|place| {
            !place.name.is_empty()
                && place.name.len() <= 80
                && (-90.0..=90.0).contains(&place.latitude)
                && (-180.0..=180.0).contains(&place.longitude)
        });
        places.truncate(5);
        return Ok(places);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the separate ToonShift-plugins checkout"]
    fn weather_package_runs_forecast_and_geocoding() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let file = root.join("ToonShift-plugins/plugins/toonshift-weather.json");
        let plugin: Plugin = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let code = plugin.wasm.unwrap();
        let forecast = br#"{"current":{"temperature_2m":-1.7,"weather_code":63},"daily":{"time":["2026-11-13"],"temperature_2m_min":[-2],"temperature_2m_max":[1],"weather_code":[85]}}"#;
        let reading: Reading =
            parse_executable_response(&code, forecast, Language::Dutch, false).unwrap();
        assert_eq!(reading.value, "-2°C");
        assert_eq!(reading.detail, "Regen");
        assert_eq!(reading.rows[0].condition, "Sneeuwbuien");
        let search = br#"{"results":[{"name":"Utrecht","latitude":52.09,"longitude":5.12,"country":"Nederland"}]}"#;
        let places: Vec<Place> =
            parse_executable_response(&code, search, Language::Dutch, true).unwrap();
        assert_eq!(places[0].name, "Utrecht, Nederland");
        assert!(
            parse_executable_response::<Reading>(&code, b"{}", Language::English, false).is_err()
        );
    }
}
