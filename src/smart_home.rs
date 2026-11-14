use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Deserialize, Serialize)]
pub struct Config {
    pub url: String,
    pub token: String,
    pub switches: Vec<String>,
}

#[cfg(any(target_os = "linux", test))]
struct Connector(Vec<u8>);

#[cfg(any(target_os = "linux", test))]
#[derive(Deserialize)]
struct Request {
    path: String,
    body: Option<serde_json::Value>,
}

#[cfg(any(target_os = "linux", test))]
impl Connector {
    #[cfg(target_os = "linux")]
    fn load(root: &Path) -> Result<Self, String> {
        let plugin = crate::plugins::catalog(root)
            .into_iter()
            .find(|plugin| {
                plugin.id == "toonshift-smart-home"
                    && plugin.publisher.as_deref().is_some_and(|publisher| {
                        publisher.eq_ignore_ascii_case(crate::plugins::OFFICIAL)
                    })
                    && matches!(plugin.source, crate::plugins::Source::SmartHome)
            })
            .and_then(|plugin| plugin.wasm)
            .ok_or("Install or update Smart Home Controls from the official Store first.")?;
        crate::plugin_runtime::decode(&plugin).map(Self)
    }

    fn plan(&self, operation: &str, id: Option<&str>, on: Option<bool>) -> Result<Request, String> {
        let input = serde_json::json!({"operation":operation,"id":id,"on":on}).to_string();
        let output = crate::plugin_runtime::render_feed(&self.0, input.as_bytes())?;
        let request: Request =
            serde_json::from_str(&output).map_err(|_| "Invalid connector request")?;
        if !request.path.starts_with("/api/")
            || request.path.len() > 256
            || !request
                .path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"/_.".contains(&byte))
            || request
                .path
                .split('/')
                .any(|part| part == "." || part == "..")
            || (operation != "set" && request.body.is_some())
        {
            return Err("Connector request exceeds its HTTPS permissions.".into());
        }
        if operation == "set" {
            let target = if on == Some(true) {
                "turn_on"
            } else {
                "turn_off"
            };
            let allowed = request.path.strip_prefix("/api/services/switch/") == Some(target)
                && request
                    .body
                    .as_ref()
                    .and_then(serde_json::Value::as_object)
                    .is_some_and(|body| {
                        body.len() == 1
                            && body.get("entity_id").and_then(serde_json::Value::as_str) == id
                    });
            if !allowed {
                return Err("The connector may only change the selected switch.".into());
            }
        }
        Ok(request)
    }
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
        self.list_with(root, &Connector::load(root)?)
    }

    #[cfg(target_os = "linux")]
    fn list_with(&self, root: &Path, connector: &Connector) -> Result<Vec<Switch>, String> {
        let request = connector.plan("list", None, None)?;
        let rows: Vec<Switch> = self.request(root, &request.path, None)?;
        Ok(rows
            .into_iter()
            .filter(|row| valid_switch(&row.entity_id))
            .collect())
    }

    #[cfg(target_os = "linux")]
    fn set(&self, root: &Path, connector: &Connector, id: &str, on: bool) -> Result<(), String> {
        if !self.switches.iter().any(|allowed| allowed == id) {
            return Err("This switch is not enabled in ToonShift.".into());
        }
        let target = if on { "on" } else { "off" };
        let command = connector.plan("set", Some(id), Some(on))?;
        let readback = connector.plan("read", Some(id), None)?;
        let _: serde_json::Value = self.request(
            root,
            &command.path,
            command.body.map(|body| body.to_string()),
        )?;
        for _ in 0..3 {
            let state: Switch = self.request(root, &readback.path, None)?;
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
        std::fs::symlink_metadata(&path).map_err(|_| "Connect Home Assistant to get started.")?;
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
    use std::io::Read;
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
    save(root, &config)
}

#[cfg(target_os = "linux")]
pub fn save(root: &Path, config: &Config) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    config.validate()?;
    let bytes = serde_json::to_vec(config).map_err(|_| "Invalid Home Assistant settings")?;
    if bytes.len() > 8192 {
        return Err("Home Assistant settings are too large".into());
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
pub fn disconnect(root: &Path) -> Result<(), String> {
    match std::fs::remove_file(root.join("home-assistant.json")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("Could not remove the Home Assistant connection.".into()),
    }
}

#[cfg(target_os = "linux")]
pub fn selected(root: &Path, change: Option<(String, bool)>) -> Result<Vec<Switch>, String> {
    let config = load(root)?;
    let connector = Connector::load(root)?;
    if let Some((id, on)) = change {
        config.set(root, &connector, &id, on)?;
    }
    let mut rows = config.list_with(root, &connector)?;
    rows.retain(|row| config.switches.contains(&row.entity_id));
    rows.sort_by(|a, b| a.entity_id.cmp(&b.entity_id));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires the separate ToonShift-plugins checkout"]
    fn installed_connector_plans_discovery_control_and_readback() {
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR")).join("ToonShift-plugins");
        let package = serde_json::from_slice(
            &std::fs::read(checkout.join("plugins/toonshift-smart-home.json")).unwrap(),
        )
        .unwrap();
        let connector = Connector(crate::plugins::checkout_module(&checkout, &package));
        assert_eq!(
            connector.plan("list", None, None).unwrap().path,
            "/api/states"
        );
        let id = Some("switch.desk");
        assert_eq!(
            connector.plan("read", id, None).unwrap().path,
            "/api/states/switch.desk"
        );
        for (on, action) in [(true, "turn_on"), (false, "turn_off")] {
            let request = connector.plan("set", id, Some(on)).unwrap();
            assert_eq!(request.path, format!("/api/services/switch/{action}"));
            assert_eq!(
                request.body.unwrap(),
                serde_json::json!({"entity_id":"switch.desk"})
            );
        }
    }

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
