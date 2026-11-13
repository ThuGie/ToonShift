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
    #[serde(skip)]
    pub previous_release: Option<String>,
    #[serde(skip)]
    pub publisher: Option<String>,
}

impl Plugin {
    fn package_id(&self) -> &str {
        self.id.rsplit('/').next().unwrap_or(&self.id)
    }

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
    #[serde(default)]
    pub value: String,
    #[serde(default)]
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
    #[serde(default)]
    pub results: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub latitude: String,
    #[serde(default)]
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
const OFFICIAL_KEY: &str = "0281447f0840d001543e4e7a51fbd4aa5da4cf57c5d9c04504d15133c4c20d6b";
const CATALOG_CACHE_LIMIT: u64 = 70000;

#[derive(Deserialize, Serialize)]
struct SignedCatalog {
    origin: String,
    manifest: String,
    signature: String,
}

fn hex_bytes<const N: usize>(value: &str) -> Option<[u8; N]> {
    let value = value.trim();
    if value.len() != N * 2 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

fn verify_manifest(key: &str, manifest: &[u8], signature: &str) -> Result<(), String> {
    use ed25519_dalek::{Signature, VerifyingKey};
    let key = hex_bytes(key)
        .and_then(|bytes| VerifyingKey::from_bytes(&bytes).ok())
        .ok_or("Invalid catalog public key")?;
    let signature =
        Signature::from_bytes(&hex_bytes(signature).ok_or("Invalid catalog signature")?);
    key.verify_strict(manifest, &signature)
        .map_err(|_| "Catalog signature verification failed".into())
}

fn catalog_key(root: &Path, owner: &str, name: &str) -> Result<String, String> {
    if format!("{owner}/{name}").eq_ignore_ascii_case(OFFICIAL) {
        return Ok(OFFICIAL_KEY.into());
    }
    let path = root.join("catalog-keys").join(format!(
        "{}--{}.pub",
        owner.to_ascii_lowercase(),
        name.to_ascii_lowercase()
    ));
    if !fs::symlink_metadata(&path).is_ok_and(|info| info.is_file() && info.len() <= 128) {
        return Err("Provision the publisher's catalog key before adding this source.".into());
    }
    fs::read_to_string(path).map_err(|error| error.to_string())
}

fn cached_repository(root: &Path, bytes: &[u8]) -> Option<Repository> {
    if let Ok(signed) = serde_json::from_slice::<SignedCatalog>(bytes) {
        let (owner, name, _) = origin_parts(&signed.origin)?;
        let key = catalog_key(root, owner, name).ok()?;
        verify_manifest(&key, signed.manifest.as_bytes(), &signed.signature).ok()?;
        let mut repository: Repository = serde_json::from_str(&signed.manifest).ok()?;
        repository.origin = Some(signed.origin);
        return Some(repository);
    }
    let repository: Repository = serde_json::from_slice(bytes).ok()?;
    if repository.origin.is_some() {
        return None;
    }
    Some(repository)
}

pub struct CatalogSource {
    path: std::path::PathBuf,
    pub repository: String,
    pub revision: String,
}

fn origin_parts(origin: &str) -> Option<(&str, &str, &str)> {
    let origin = origin
        .strip_prefix("https://raw.githubusercontent.com/")?
        .strip_suffix('/')?;
    let mut parts = origin.split('/');
    let (owner, name, revision) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || owner.is_empty()
        || name.is_empty()
        || revision.is_empty()
        || ![owner, name, revision].iter().all(|part| {
            part.as_bytes()[0].is_ascii_alphanumeric()
                && part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
        })
    {
        return None;
    }
    Some((owner, name, revision))
}

pub fn sources(root: &Path) -> Vec<CatalogSource> {
    let mut sources: Vec<_> = fs::read_dir(root.join("catalog"))
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != "json"
                || !fs::symlink_metadata(&path)
                    .is_ok_and(|info| info.is_file() && info.len() <= CATALOG_CACHE_LIMIT)
            {
                return None;
            }
            let bytes = fs::read(&path).ok()?;
            let origin = serde_json::from_slice::<SignedCatalog>(&bytes)
                .ok()
                .map(|signed| signed.origin)
                .or_else(|| serde_json::from_slice::<Repository>(&bytes).ok()?.origin)?;
            let (owner, name, revision) = origin_parts(&origin)?;
            if path.file_name()?.to_str()?
                != format!(
                    "{}--{}.json",
                    owner.to_ascii_lowercase(),
                    name.to_ascii_lowercase()
                )
            {
                return None;
            }
            Some(CatalogSource {
                path,
                repository: format!("{owner}/{name}"),
                revision: revision.into(),
            })
        })
        .collect();
    sources.sort_by(|a, b| {
        a.repository
            .to_ascii_lowercase()
            .cmp(&b.repository.to_ascii_lowercase())
    });
    sources
}

pub fn remove_source(source: &CatalogSource) -> io::Result<()> {
    fs::remove_file(&source.path)
}

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
    let key = catalog_key(root, owner, name)?;
    let origin = format!("https://raw.githubusercontent.com/{owner}/{name}/{revision}/");
    let ca = root.join("https-roots.pem");
    let response = crate::download::get(&ca, &format!("{origin}catalog.json"))?;
    let signature = crate::download::get(&ca, &format!("{origin}catalog.sig"))?;
    let signature = String::from_utf8(signature).map_err(|_| "Invalid catalog signature")?;
    verify_manifest(&key, &response, &signature)?;
    let repository: Repository =
        serde_json::from_slice(&response).map_err(|_| "Invalid catalog manifest")?;
    if repository.version != 1 || repository.plugins.len() > 64 {
        return Err("Unsupported catalog version or plugin count.".into());
    }
    let mut seen = HashSet::new();
    if !repository
        .plugins
        .iter()
        .all(|plugin| valid_plugin(plugin) && plugin.artifact.is_some() && seen.insert(&plugin.id))
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
    let cached = serde_json::to_vec(&SignedCatalog {
        origin,
        manifest: String::from_utf8(response).map_err(|_| "Invalid catalog manifest encoding")?,
        signature,
    })
    .map_err(|error| error.to_string())?;
    if cached.len() as u64 > CATALOG_CACHE_LIMIT {
        return Err("Catalog exceeds cache size limit".into());
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
    let id = plugin.package_id();
    let path = artifact
        .package_file(root, id)
        .ok_or("Invalid plugin package origin")?;
    if !artifact.valid() {
        return Err("Invalid plugin package origin".into());
    }
    let response = crate::download::package(
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
        || package.id != id
        || package.release != plugin.release
    {
        return Err("Plugin package does not match the catalog entry".into());
    }
    if let Some(code) = &package.wasm {
        crate::plugin_runtime::validate(
            &crate::plugin_runtime::decode(code)?,
            matches!(package.source, Source::HttpsJson),
        )?;
    }
    let dir = root.join("packages");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary).map_err(|err| err.to_string())?;
    file.write_all(&response)
        .and_then(|_| file.sync_all())
        .map_err(|err| err.to_string())?;
    if let Some((bytes, current)) = read_package(&path, id) {
        if current.release != package.release {
            let previous = path.with_extension("previous");
            let staged = path.with_extension("previous.tmp");
            let mut file = File::create(&staged).map_err(|err| err.to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .and_then(|_| fs::rename(staged, previous))
                .map_err(|err| err.to_string())?;
        }
    }
    fs::rename(temporary, path).map_err(|err| err.to_string())
}

pub fn rollback(root: &Path, plugin: &Plugin) -> Result<(), String> {
    let id = plugin.package_id();
    let path = plugin
        .artifact
        .as_ref()
        .and_then(|artifact| artifact.package_file(root, id))
        .ok_or("Invalid plugin package origin")?;
    let previous = path.with_extension("previous");
    let (_, candidate) = read_package(&previous, id).ok_or("No previous plugin version")?;
    if let Some(code) = candidate.wasm {
        crate::plugin_runtime::validate(
            &crate::plugin_runtime::decode(&code)?,
            matches!(candidate.source, Source::HttpsJson),
        )?;
    }
    fs::rename(previous, path).map_err(|error| error.to_string())
}

fn read_package(path: &Path, id: &str) -> Option<(Vec<u8>, Plugin)> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > crate::download::PACKAGE_LIMIT as u64 {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let plugin: Plugin = serde_json::from_slice(&bytes).ok()?;
    (plugin.id == id && plugin.artifact.is_none() && valid_plugin(&plugin))
        .then_some((bytes, plugin))
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
        && plugin.wasm.as_ref().is_none_or(|code| {
            matches!(plugin.source, Source::Wasm | Source::HttpsJson)
                && crate::plugin_runtime::decode(code).is_ok()
        })
        && plugin.artifact.as_ref().is_none_or(Artifact::valid)
        && (plugin.artifact.is_some()
            || ((!matches!(plugin.source, Source::BoxTalk) || plugin.metric.is_some())
                && (!matches!(plugin.source, Source::MeterArchive) || plugin.meter.is_some())
                && (!matches!(plugin.source, Source::Wasm) || plugin.wasm.is_some())
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
        if !fs::symlink_metadata(&path)
            .is_ok_and(|entry| entry.is_file() && entry.len() <= CATALOG_CACHE_LIMIT)
        {
            continue;
        }
        let candidates = fs::read(&path).ok().and_then(|bytes| {
            if let Some(repository) = cached_repository(root, &bytes) {
                return (repository.version == 1 && repository.plugins.len() <= 64).then(|| {
                    repository
                        .plugins
                        .into_iter()
                        .map(|mut plugin| {
                            plugin.publisher = repository
                                .origin
                                .as_deref()
                                .and_then(origin_parts)
                                .map(|(owner, name, _)| format!("{owner}/{name}"));
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
            let valid = valid_plugin(&plugin);
            let package_id = plugin.id.clone();
            if let Some(publisher) = plugin
                .publisher
                .as_ref()
                .filter(|publisher| !publisher.eq_ignore_ascii_case(OFFICIAL))
            {
                plugin.id = format!("{}/{}", publisher.to_ascii_lowercase(), package_id);
            }
            let valid = valid && seen.insert(plugin.id.clone());
            if valid {
                if let Some(package) = plugin
                    .artifact
                    .as_ref()
                    .and_then(|artifact| artifact.package_file(root, &package_id))
                {
                    let installed =
                        read_package(&package, &package_id).filter(|(bytes, installed)| {
                            use sha2::{Digest, Sha256};
                            installed.release != plugin.release
                                || plugin.artifact.as_ref().is_some_and(|artifact| {
                                    format!("{:x}", Sha256::digest(bytes))
                                        .eq_ignore_ascii_case(&artifact.sha256)
                                })
                        });
                    if let Some((_, installed)) = installed {
                        plugin.previous_release =
                            read_package(&package.with_extension("previous"), &package_id)
                                .map(|(_, previous)| previous.release)
                                .filter(|release| release != &installed.release);
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
    pub dashboard_names: Vec<String>,
    pub wide_tiles: bool,
    pub locations: std::collections::HashMap<String, String>,
    pub custom_locations: std::collections::HashMap<String, Place>,
    pub language: crate::language::Language,
    pub day_start: u8,
    pub night_start: u8,
    pub day_brightness: u8,
    pub night_brightness: u8,
    pub idle_minutes: u8,
    pub idle_dashboard_page: Option<usize>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Midnight,
            meter_sources: Default::default(),
            energy_rates: Default::default(),
            appearance: Appearance::Auto,
            installed: vec!["clock".into(), "boiler".into()],
            dashboard_names: vec![String::new(); 16],
            wide_tiles: false,
            locations: Default::default(),
            custom_locations: Default::default(),
            language: Default::default(),
            day_start: 7,
            night_start: 20,
            day_brightness: 95,
            night_brightness: 30,
            idle_minutes: 0,
            idle_dashboard_page: None,
        }
    }
}

impl Settings {
    pub fn migrate_plugin_ids(&mut self, catalog: &[Plugin]) -> bool {
        let ids: HashSet<_> = self
            .installed
            .iter()
            .chain(self.locations.keys())
            .chain(self.custom_locations.keys())
            .cloned()
            .collect();
        let mut changed = false;
        for id in ids {
            if id.contains('/') || catalog.iter().any(|plugin| plugin.id == id) {
                continue;
            }
            let mut matches = catalog.iter().filter(|plugin| {
                plugin.package_id() == id
                    && (plugin.artifact.is_none() || plugin.installed_release.is_some())
            });
            let Some(plugin) = matches.next().filter(|_| matches.next().is_none()) else {
                continue;
            };
            for tile in self.installed.iter_mut().filter(|tile| **tile == id) {
                tile.clone_from(&plugin.id);
            }
            if let Some(location) = self.locations.remove(&id) {
                self.locations.entry(plugin.id.clone()).or_insert(location);
            }
            if let Some(location) = self.custom_locations.remove(&id) {
                self.custom_locations
                    .entry(plugin.id.clone())
                    .or_insert(location);
            }
            changed = true;
        }
        changed
    }

    pub fn tiles_per_page(&self) -> usize {
        if self.wide_tiles {
            2
        } else {
            4
        }
    }

    pub fn dashboard_pages(&self) -> usize {
        self.installed.len().div_ceil(self.tiles_per_page())
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the separate ToonShift-plugins checkout"]
    fn official_signature_rejects_manifest_changes() {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("ToonShift-plugins");
        let manifest = fs::read_to_string(checkout.join("catalog.json"))
            .unwrap()
            .replace("\r\n", "\n");
        let signature = fs::read_to_string(checkout.join("catalog.sig")).unwrap();
        assert!(verify_manifest(OFFICIAL_KEY, manifest.as_bytes(), &signature).is_ok());
        let modified = manifest.replace("Weather", "Weather Changed");
        assert!(verify_manifest(OFFICIAL_KEY, modified.as_bytes(), &signature).is_err());
        let signed = SignedCatalog {
            origin: format!("https://raw.githubusercontent.com/{OFFICIAL}/main/"),
            manifest,
            signature,
        };
        let bytes = serde_json::to_vec(&signed).unwrap();
        assert!(cached_repository(&checkout, &bytes).is_some());
        assert!(cached_repository(&checkout, br#"{"version":1,"plugins":[],"origin":"https://raw.githubusercontent.com/ThuGie/ToonShift-plugins/main/"}"#).is_none());
    }

    #[test]
    fn migration_keeps_order_and_leaves_ambiguous_ids_alone() {
        let community = |id: &str| {
            let mut plugin: Plugin = serde_json::from_str(
                r#"{"id":"widget","title":"Widget","description":"","source":"note"}"#,
            )
            .unwrap();
            plugin.id = id.into();
            plugin
        };
        let mut settings = Settings {
            installed: vec!["widget".into(), "clock".into(), "widget".into()],
            ..Settings::default()
        };
        settings.locations.insert("widget".into(), "Utrecht".into());
        let first = community("alice/tools/widget");
        let second = community("bob/widgets/widget");
        assert!(!settings.migrate_plugin_ids(&[first.clone(), second]));
        assert!(settings.migrate_plugin_ids(&[first]));
        assert_eq!(
            settings.installed,
            ["alice/tools/widget", "clock", "alice/tools/widget"]
        );
        assert_eq!(settings.locations["alice/tools/widget"], "Utrecht");
    }
}
