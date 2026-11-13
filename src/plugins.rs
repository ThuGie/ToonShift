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
    MeterArchive,
    Wasm,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
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
    pub title_nl: Option<String>,
    pub description: String,
    pub source: Source,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub release: String,
    pub feed: Option<Feed>,
    pub metric: Option<Metric>,
    pub meter: Option<crate::energy::Meter>,
    pub wasm: Option<String>,
    pub artifact: Option<Artifact>,
    #[serde(skip)]
    pub installed_release: Option<String>,
}

impl Plugin {
    pub fn title(&self, language: crate::language::Language) -> &str {
        if language == crate::language::Language::Dutch {
            return self
                .title_nl
                .as_deref()
                .unwrap_or_else(|| language.translate(&self.title));
        }
        &self.title
    }
}

#[derive(Clone, Deserialize)]
pub struct Artifact {
    path: String,
    sha256: String,
    #[serde(skip)]
    origin: Option<String>,
}

impl Artifact {
    fn valid(&self) -> bool {
        self.path.starts_with("plugins/")
            && self.path.ends_with(".json")
            && self.path.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
            && self.sha256.len() == 64
            && self.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    }

    fn package_file(&self, root: &Path, id: &str) -> Option<std::path::PathBuf> {
        let origin = self
            .origin
            .as_deref()?
            .strip_prefix("https://raw.githubusercontent.com/")?;
        let mut parts = origin.split('/');
        let (owner, repository, revision) = (parts.next()?, parts.next()?, parts.next()?);
        let name = |value: &str| {
            !value.is_empty()
                && value != "."
                && value != ".."
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        };
        if !name(owner)
            || owner.contains("--")
            || !name(repository)
            || !name(revision)
            || parts.next() != Some("")
            || parts.next().is_some()
        {
            return None;
        }
        Some(root.join("packages").join(format!(
            "{}--{}--{id}.json",
            owner.to_ascii_lowercase(),
            repository.to_ascii_lowercase()
        )))
    }
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
    #[serde(default)]
    pub labels_nl: std::collections::HashMap<String, String>,
    #[serde(default = "default_decimals")]
    pub decimals: usize,
    #[serde(alias = "forecast")]
    pub series: Option<RangeSeries>,
    pub places: Vec<Place>,
    pub search: Option<LocationSearch>,
}

#[derive(Clone, Deserialize)]
pub struct LocationSearch {
    pub url: String,
    pub results: String,
    pub name: String,
    pub latitude: String,
    pub longitude: String,
    #[serde(default)]
    pub region: Vec<String>,
}

#[derive(Clone, Deserialize)]
pub struct RangeSeries {
    pub dates: String,
    pub min: String,
    pub max: String,
    pub codes: String,
}

fn default_decimals() -> usize {
    1
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
    origin: Option<String>,
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
    let mut cached: serde_json::Value =
        serde_json::from_slice(&response).map_err(|_| "Invalid catalog manifest")?;
    cached["origin"] = serde_json::json!(format!(
        "https://raw.githubusercontent.com/{owner}/{name}/{revision}/"
    ));
    let cached = serde_json::to_vec(&cached).map_err(|err| err.to_string())?;
    if cached.len() > 32768 {
        return Err("Catalog exceeds 32 KiB".into());
    }
    let result = (|| -> io::Result<()> {
        let mut handle = File::create(&temporary)?;
        handle.write_all(&cached)?;
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

pub fn install(root: &Path, plugin: &Plugin) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let artifact = plugin
        .artifact
        .as_ref()
        .ok_or("Plugin has no downloadable package")?;
    let origin = artifact
        .origin
        .as_ref()
        .ok_or("Refresh the catalog before installing this plugin")?;
    let path = artifact
        .package_file(root, &plugin.id)
        .ok_or("Invalid plugin package origin")?;
    if !artifact.valid() {
        return Err("Invalid plugin package origin".into());
    }
    let response = crate::download::get(
        &root.join("https-roots.pem"),
        &format!("{origin}{}", artifact.path),
    )?;
    if !format!("{:x}", Sha256::digest(&response)).eq_ignore_ascii_case(&artifact.sha256) {
        return Err("Plugin package checksum does not match the catalog".into());
    }
    let package: Plugin =
        serde_json::from_slice(&response).map_err(|_| "Invalid plugin package")?;
    if !valid_plugin(&package)
        || package.artifact.is_some()
        || package.id != plugin.id
        || package.release != plugin.release
    {
        return Err("Plugin package does not match the catalog entry".into());
    }
    let dir = root.join("packages");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary).map_err(|err| err.to_string())?;
    file.write_all(&response)
        .and_then(|_| file.sync_all())
        .map_err(|err| err.to_string())?;
    fs::rename(temporary, path).map_err(|err| err.to_string())
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
        && plugin
            .title_nl
            .as_ref()
            .is_none_or(|title| !title.trim().is_empty() && title.len() <= 96)
        && plugin.description.len() <= 256
        && plugin.text.len() <= 1024
        && plugin.artifact.as_ref().is_none_or(Artifact::valid)
        && (plugin.artifact.is_some()
            || ((!matches!(plugin.source, Source::BoxTalk) || plugin.metric.is_some())
                && (!matches!(plugin.source, Source::MeterArchive) || plugin.meter.is_some())
                && (!matches!(plugin.source, Source::Wasm)
                    || plugin
                        .wasm
                        .as_ref()
                        .is_some_and(|code| crate::plugin_runtime::decode(code).is_ok()))
                && (!matches!(plugin.source, Source::HttpsJson)
                    || plugin.feed.as_ref().is_some_and(|feed| {
                        (1..=24).contains(&feed.places.len())
                            && feed.url.len() <= 1024
                            && feed.decimals <= 3
                            && feed.places.iter().all(|place| {
                                !place.name.is_empty()
                                    && place.name.len() <= 80
                                    && (-90.0..=90.0).contains(&place.latitude)
                                    && (-180.0..=180.0).contains(&place.longitude)
                            })
                    }))))
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
                return (repository.version == 1 && repository.plugins.len() <= 64).then(|| {
                    repository
                        .plugins
                        .into_iter()
                        .map(|mut plugin| {
                            if let Some(artifact) = &mut plugin.artifact {
                                artifact.origin = repository.origin.clone();
                            }
                            plugin
                        })
                        .collect()
                });
            }
            serde_json::from_slice::<Plugin>(&bytes)
                .ok()
                .map(|plugin| vec![plugin])
        });
        let Some(candidates) = candidates else {
            eprintln!("Ignoring invalid catalog file {}", path.display());
            continue;
        };
        for mut plugin in candidates {
            let valid = valid_plugin(&plugin) && seen.insert(plugin.id.clone());
            if valid {
                if let Some(package) = plugin
                    .artifact
                    .as_ref()
                    .and_then(|artifact| artifact.package_file(root, &plugin.id))
                {
                    if let Some(installed) = fs::read(package)
                        .ok()
                        .filter(|bytes| bytes.len() <= 32768)
                        .and_then(|bytes| serde_json::from_slice::<Plugin>(&bytes).ok())
                        .filter(|installed| {
                            installed.id == plugin.id
                                && installed.artifact.is_none()
                                && valid_plugin(installed)
                        })
                    {
                        plugin.installed_release = Some(installed.release);
                        plugin.feed = installed.feed;
                        plugin.metric = installed.metric;
                        plugin.meter = installed.meter;
                        plugin.wasm = installed.wasm;
                        plugin.text = installed.text;
                        plugin.source = installed.source;
                    }
                }
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
    pub meter_sources: crate::advanced::MeterSources,
    pub energy_rates: crate::energy::Rates,
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
            meter_sources: Default::default(),
            energy_rates: Default::default(),
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
