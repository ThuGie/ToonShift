use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Deserialize, Serialize)]
pub struct Config {
    pub url: String,
    pub token: String,
    pub switches: Vec<String>,
}

#[derive(Deserialize)]
pub struct Switch {
    pub entity_id: String,
    pub state: String,
    attributes: Attributes,
}

#[derive(Deserialize)]
struct Attributes {
    friendly_name: Option<String>,
}

impl Switch {
    pub fn name(&self) -> &str {
        self.attributes
            .friendly_name
            .as_deref()
            .unwrap_or(&self.entity_id)
    }
}

pub fn valid_switch(id: &str) -> bool {
    id.strip_prefix("switch.").is_some_and(|name| {
        (1..=96).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    })
}

impl Config {
    pub fn validate(&self) -> Result<(), String> {
        let host = self
            .url
            .trim_end_matches('/')
            .strip_prefix("https://")
            .unwrap_or_default();
        if host.is_empty()
            || host.len() > 253
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".:-".contains(&b))
            || !(20..=4096).contains(&self.token.len())
            || !self
                .token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            || self.switches.len() > 32
            || !self.switches.iter().all(|id| valid_switch(id))
            || self
                .switches
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != self.switches.len()
        {
            return Err(
                "Use an HTTPS Home Assistant URL, token, and up to 32 unique switch IDs.".into(),
            );
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn request<T: serde::de::DeserializeOwned>(
        &self,
        root: &Path,
        route: &str,
        body: Option<String>,
    ) -> Result<T, String> {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};
        let mut command = Command::new("/usr/bin/curl");
        command.args([
            "-q",
            "-fsS",
            "--globoff",
            "--proto",
            "=https",
            "--noproxy",
            "*",
            "--connect-timeout",
            "3",
            "--max-time",
            "6",
            "--config",
            "-",
        ]);
        let ca = root.join("home-assistant-ca.pem");
        if ca.is_file() {
            command.arg("--cacert").arg(ca);
        }
        if let Some(body) = &body {
            command.args([
                "--header",
                "Content-Type: application/json",
                "--data-binary",
                body,
            ]);
        }
        let url = format!("{}{route}", self.url.trim_end_matches('/'));
        let mut child = command
            .args(["--", &url])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Could not start Home Assistant request")?;
        let input =
            child.stdin.take().unwrap().write_all(
                format!("header = \"Authorization: Bearer {}\"\n", self.token).as_bytes(),
            );
        let mut output = Vec::new();
        let read = child
            .stdout
            .take()
            .unwrap()
            .take(1_048_577)
            .read_to_end(&mut output);
        if input.is_err() || read.is_err() || output.len() > 1_048_576 {
            let _ = child.kill();
        }
        if !child.wait().is_ok_and(|status| status.success())
            || input.is_err()
            || read.is_err()
            || output.len() > 1_048_576
        {
            return Err(
                "Home Assistant request failed. Check HTTPS, the token and device permissions."
                    .into(),
            );
        }
        serde_json::from_slice(&output).map_err(|_| "Invalid Home Assistant response".into())
    }

    #[cfg(target_os = "linux")]
    pub fn list(&self, root: &Path) -> Result<Vec<Switch>, String> {
        let rows: Vec<Switch> = self.request(root, "/api/states", None)?;
        Ok(rows
            .into_iter()
            .filter(|row| valid_switch(&row.entity_id))
            .collect())
    }

    #[cfg(target_os = "linux")]
    pub fn set(&self, root: &Path, id: &str, on: bool) -> Result<(), String> {
        if !self.switches.iter().any(|allowed| allowed == id) {
            return Err("This switch is not enabled in ToonShift.".into());
        }
        let target = if on { "on" } else { "off" };
        let _: serde_json::Value = self.request(
            root,
            &format!("/api/services/switch/turn_{target}"),
            Some(serde_json::json!({"entity_id": id}).to_string()),
        )?;
        for _ in 0..3 {
            let state: Switch = self.request(root, &format!("/api/states/{id}"), None)?;
            if state.entity_id == id && state.state == target {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(350));
        }
        Err("Home Assistant did not confirm the switch state.".into())
    }
}

#[cfg(target_os = "linux")]
pub fn load(root: &Path) -> Result<Config, String> {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("home-assistant.json");
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| "Configure Home Assistant over SSH first.")?;
    if !metadata.is_file() || metadata.len() > 8192 || metadata.permissions().mode() & 0o077 != 0 {
        return Err("Home Assistant configuration must be a private file.".into());
    }
    let config: Config = serde_json::from_slice(
        &std::fs::read(path).map_err(|_| "Could not read Home Assistant settings")?,
    )
    .map_err(|_| "Invalid Home Assistant settings")?;
    config.validate()?;
    Ok(config)
}

#[cfg(target_os = "linux")]
pub fn setup(root: &Path) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read setup input")?;
    if bytes.len() > 8192 {
        return Err("Home Assistant settings are too large".into());
    }
    let config: Config =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid Home Assistant settings")?;
    config.validate()?;
    let switches = config.list(root)?;
    if config.switches.is_empty() {
        for switch in switches {
            println!("{}", switch.entity_id);
        }
        return Ok(());
    }
    if !config
        .switches
        .iter()
        .all(|id| switches.iter().any(|switch| switch.entity_id == *id))
    {
        return Err("One or more selected switches were not found in Home Assistant".into());
    }
    let temporary = root.join(format!(".home-assistant-{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .mode(0o600)
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let save = file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| std::fs::rename(&temporary, root.join("home-assistant.json")));
    if let Err(error) = save {
        let _ = std::fs::remove_file(temporary);
        return Err(error.to_string());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn selected(root: &Path, change: Option<(String, bool)>) -> Result<Vec<Switch>, String> {
    let config = load(root)?;
    if let Some((id, on)) = change {
        config.set(root, &id, on)?;
    }
    let mut rows = config.list(root)?;
    rows.retain(|row| config.switches.contains(&row.entity_id));
    rows.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_requires_https_and_limits_control_to_unique_switches() {
        let mut config = Config {
            url: "https://ha.example.net:8123".into(),
            token: "x".repeat(40),
            switches: vec!["switch.desk_plug".into()],
        };
        assert!(config.validate().is_ok());
        config.switches.push("switch.desk_plug".into());
        assert!(config.validate().is_err());
        config.switches.pop();
        config.url = "http://ha.example.net".into();
        assert!(config.validate().is_err());
        assert!(!valid_switch("switch.foo/../../light"));
        assert!(!valid_switch("lock.front_door"));
    }
}
