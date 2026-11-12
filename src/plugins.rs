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
}

#[derive(Clone, Deserialize)]
pub struct Plugin {
    pub id: String,
    pub title: String,
    pub description: String,
    pub source: Source,
    #[serde(default)]
    pub text: String,
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
            let valid = !plugin.id.is_empty()
                && plugin.id.len() <= 40
                && plugin
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && !plugin.title.trim().is_empty()
                && plugin.title.len() <= 96
                && plugin.description.len() <= 256
                && plugin.text.len() <= 1024
                && seen.insert(plugin.id.clone());
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Midnight,
            appearance: Appearance::Auto,
            installed: vec!["clock".into(), "boiler".into()],
        }
    }
}

impl Settings {
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
