use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Clock,
    Boiler,
    NextTarget,
    Note,
    File,
    HttpsJson,
    BoxTalk,
}

#[derive(Clone, Copy, Debug, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    RoomHumidity,
    RoomTemperature,
    Electricity,
    Gas,
}

#[derive(Clone, Deserialize)]
pub struct Plugin {
    pub id: String,
    pub title: String,
    pub description: String,
    pub source: Source,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub release: String,
    pub feed: Option<Feed>,
    pub metric: Option<Metric>,
}

#[derive(Clone, Deserialize)]
pub struct Feed {
    pub url: String,
    pub value: String,
    pub suffix: String,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub labels: std::collections::HashMap<String, String>,
    pub forecast: Option<Forecast>,
    pub places: Vec<Place>,
}

#[derive(Clone, Deserialize)]
pub struct Forecast {
    pub dates: String,
    pub min: String,
    pub max: String,
    pub codes: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

const BUILTINS: &str = r#"[
    {"id":"clock","title":"Clock","description":"Local time","source":"clock"},
    {"id":"boiler","title":"Boiler","description":"Connection and burner state","source":"boiler"},
    {"id":"next-target","title":"Next target","description":"Upcoming schedule temperature","source":"next_target"},
    {"id":"quick-note","title":"Quick note","description":"A note on your dashboard","source":"note","text":"Welcome home"}
]"#;

#[derive(Deserialize)]
struct Repository {
    version: u8,
    plugins: Vec<Plugin>,
}

pub const OFFICIAL: &str = "ThuGie/ToonShift-plugins";

pub fn sync(root: &Path, input: &str) -> Result<usize, String> {
    let input = input.trim();
    let (repository, revision) = input.split_once('@').unwrap_or((input, "main"));
    let (owner, name) = repository
        .split_once('/')
        .ok_or("Enter owner/repository.")?;
    let valid = |part: &str, max, dot| {
        (1..=max).contains(&part.len())
            && part.as_bytes()[0].is_ascii_alphanumeric()
            && part.bytes().all(|c| {
                c.is_ascii_alphanumeric() || c == b'-' || (dot && matches!(c, b'_' | b'.'))
            })
    };
    if !valid(owner, 39, false) || owner.contains("--") || !valid(name, 100, true) {
        return Err("Enter a valid GitHub owner and repository name.".into());
    }
    if revision != "main"
        && (revision.len() != 40 || !revision.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("Use @ followed by a full 40-digit GitHub commit.".into());
    }
    let url = format!("https://raw.githubusercontent.com/{owner}/{name}/{revision}/catalog.json");
    let response = crate::download::get(&root.join("https-roots.pem"), &url)?;
    let repository: Repository =
        serde_json::from_slice(&response).map_err(|_| "Invalid catalog manifest")?;
    if repository.version != 1 || repository.plugins.len() > 64 {
        return Err("Unsupported catalog version or plugin count.".into());
    }
    let mut seen = HashSet::new();
    if !repository
        .plugins
        .iter()
        .all(|plugin| valid_plugin(plugin) && seen.insert(&plugin.id))
    {
        return Err("The catalog contains invalid or duplicate widgets.".into());
    }
    let dir = root.join("catalog");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = format!(
        "{}--{}",
        owner.to_ascii_lowercase(),
        name.to_ascii_lowercase()
    );
    let temporary = dir.join(format!(".{file}.{}.tmp", std::process::id()));
    let destination = dir.join(format!("{file}.json"));
    let result = (|| -> io::Result<()> {
        let mut handle = File::create(&temporary)?;
        handle.write_all(&response)?;
        handle.sync_all()?;
        fs::rename(&temporary, destination)?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(temporary);
        return Err(error.to_string());
    }
    Ok(repository.plugins.len())
}

fn valid_plugin(plugin: &Plugin) -> bool {
    !plugin.id.is_empty()
        && plugin.id.len() <= 40
        && plugin
            .id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !plugin.title.trim().is_empty()
        && plugin.title.len() <= 96
        && plugin.description.len() <= 256
        && plugin.text.len() <= 1024
        && (!matches!(plugin.source, Source::BoxTalk) || plugin.metric.is_some())
        && (!matches!(plugin.source, Source::HttpsJson)
            || plugin.feed.as_ref().is_some_and(|feed| {
                (1..=24).contains(&feed.places.len())
                    && feed.url.len() <= 1024
                    && feed.places.iter().all(|place| {
                        !place.name.is_empty()
                            && place.name.len() <= 80
                            && (-90.0..=90.0).contains(&place.latitude)
                            && (-180.0..=180.0).contains(&place.longitude)
                    })
            }))
}

pub fn catalog(root: &Path) -> Vec<Plugin> {
    let mut plugins: Vec<Plugin> = serde_json::from_str(BUILTINS).expect("built-in catalog");
    let mut entries: Vec<_> = fs::read_dir(root.join("catalog"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort();
    let mut seen: HashSet<_> = plugins.iter().map(|plugin| plugin.id.clone()).collect();
    for path in entries.into_iter().take(64) {
        if !fs::symlink_metadata(&path).is_ok_and(|entry| entry.is_file() && entry.len() <= 32768) {
            continue;
        }
        let candidates = fs::read(&path).ok().and_then(|bytes| {
            if let Ok(repository) = serde_json::from_slice::<Repository>(&bytes) {
                return (repository.version == 1 && repository.plugins.len() <= 64)
                    .then_some(repository.plugins);
            }
            serde_json::from_slice::<Plugin>(&bytes)
                .ok()
                .map(|plugin| vec![plugin])
        });
        let Some(candidates) = candidates else {
            eprintln!("Ignoring invalid catalog file {}", path.display());
            continue;
        };
        for plugin in candidates {
            let valid = valid_plugin(&plugin) && seen.insert(plugin.id.clone());
            if valid {
                plugins.push(plugin);
            } else {
                eprintln!(
                    "Ignoring invalid or duplicate plugin {} from {}",
                    plugin.id,
                    path.display()
                );
            }
            if plugins.len() >= 128 {
                return plugins;
            }
        }
    }
    plugins
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Midnight,
    Forest,
    Plum,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    Auto,
    Day,
    Night,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub appearance: Appearance,
    pub installed: Vec<String>,
    pub locations: std::collections::HashMap<String, String>,
    pub custom_locations: std::collections::HashMap<String, Place>,
    pub language: crate::language::Language,
    pub day_start: u8,
    pub night_start: u8,
    pub day_brightness: u8,
    pub night_brightness: u8,
    pub idle_minutes: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Midnight,
            appearance: Appearance::Auto,
            installed: vec!["clock".into(), "boiler".into()],
            locations: Default::default(),
            custom_locations: Default::default(),
            language: Default::default(),
            day_start: 7,
            night_start: 20,
            day_brightness: 95,
            night_brightness: 30,
            idle_minutes: 0,
        }
    }
}

impl Settings {
    pub fn daylight(&self, hour: u8) -> bool {
        match self.appearance {
            Appearance::Day => true,
            Appearance::Night => false,
            Appearance::Auto if self.day_start < self.night_start => {
                (self.day_start..self.night_start).contains(&hour)
            }
            Appearance::Auto => hour >= self.day_start || hour < self.night_start,
        }
    }

    pub fn load(root: &Path) -> Self {
        match fs::read(root.join("settings.json")) {
            Ok(data) => serde_json::from_slice(&data).unwrap_or_else(|error| {
                eprintln!("Could not load settings: {error}");
                Self::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                eprintln!("Could not read settings: {error}");
                Self::default()
            }
        }
    }

    pub fn save(&self, root: &Path) -> io::Result<()> {
        let temporary = root.join("settings.tmp");
        let mut file = File::create(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.sync_all()?;
        fs::rename(temporary, root.join("settings.json"))?;
        #[cfg(unix)]
        File::open(root)?.sync_all()?;
        Ok(())
    }
}
