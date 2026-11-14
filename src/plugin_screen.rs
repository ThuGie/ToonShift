use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default, Deserialize)]
pub struct Screen {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub buttons: Vec<Button>,
}

#[derive(Deserialize)]
pub struct Field {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub readonly: bool,
}

#[derive(Deserialize)]
pub struct Button {
    pub id: String,
    pub label: String,
}

#[derive(Deserialize)]
struct Output {
    screen: Option<Screen>,
    request: Option<Request>,
    #[serde(default)]
    state: Value,
    #[serde(default)]
    save: bool,
}

#[derive(Deserialize)]
struct Request {
    host: String,
    path: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<String>,
    array: Option<ArraySelection>,
}

#[derive(Deserialize)]
struct ArraySelection {
    prefix_pointer: String,
    prefix: String,
    pointers: Vec<String>,
    #[serde(default)]
    offset: usize,
    limit: usize,
}

impl ArraySelection {
    fn apply(&self, value: Value) -> Result<Value, String> {
        let pointer = |value: &str| value.starts_with('/') && value.len() <= 80;
        if !(1..=32).contains(&self.limit)
            || self.offset > 4096
            || !pointer(&self.prefix_pointer)
            || !(1..=8).contains(&self.pointers.len())
            || !self.pointers.iter().all(|value| pointer(value))
            || self.prefix.len() > 80
        {
            return Err("Invalid HTTPS array selection".into());
        }
        let rows = value.as_array().ok_or("Expected an HTTPS array response")?;
        Ok(Value::Array(
            rows.iter()
                .filter(|row| {
                    row.pointer(&self.prefix_pointer)
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.starts_with(&self.prefix))
                })
                .skip(self.offset)
                .take(self.limit)
                .map(|row| {
                    Value::Array(
                        self.pointers
                            .iter()
                            .map(|pointer| row.pointer(pointer).cloned().unwrap_or_default())
                            .collect(),
                    )
                })
                .collect(),
        ))
    }
}

impl Screen {
    fn valid(&self) -> bool {
        let id = |id: &str| {
            (1..=48).contains(&id.len())
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        };
        let label =
            |label: &str| (1..=96).contains(&label.len()) && !label.chars().any(char::is_control);
        self.fields.len() <= 12
            && self.buttons.len() <= 48
            && self.message.len() <= 512
            && !self.message.chars().any(char::is_control)
            && self.fields.iter().enumerate().all(|(index, field)| {
                id(&field.id)
                    && label(&field.label)
                    && !field.value.chars().any(char::is_control)
                    && field.value.len() <= 4096
                    && self.fields[..index]
                        .iter()
                        .all(|other| other.id != field.id)
            })
            && self.buttons.iter().enumerate().all(|(index, button)| {
                id(&button.id)
                    && label(&button.label)
                    && self.buttons[..index]
                        .iter()
                        .all(|other| other.id != button.id)
            })
    }
}

pub struct Event {
    pub action: String,
    pub language: &'static str,
    pub fields: BTreeMap<String, String>,
    pub state: Value,
}

#[cfg(any(target_os = "linux", test))]
fn run(
    code: &[u8],
    event: Event,
    mut send: impl FnMut(&Request) -> Result<Vec<u8>, String>,
) -> Result<(Screen, Value, bool), String> {
    let user_action = event.action != "open";
    let mut input = serde_json::json!({
        "version": 1, "action": event.action, "language": event.language,
        "fields": event.fields, "state": event.state,
        "timestamp_ms": timestamp_ms(),
    });
    for step in 0..=6 {
        let output = crate::plugin_runtime::render_feed(code, input.to_string().as_bytes())?;
        let output: Output =
            serde_json::from_str(&output).map_err(|_| "Invalid plugin response")?;
        match (output.screen, output.request) {
            (Some(screen), None) if screen.valid() && (!output.save || user_action) => {
                return Ok((screen, output.state, output.save))
            }
            (None, Some(request)) if user_action && !output.save && step < 6 => {
                request.validate_path()?;
                let response = send(&request)
                    .and_then(|bytes| {
                        serde_json::from_slice::<Value>(&bytes)
                            .map_err(|_| "Invalid JSON response".into())
                    })
                    .and_then(|response| {
                        if let Some(selection) = request.array {
                            selection.apply(response)
                        } else {
                            Ok(response)
                        }
                    });
                input["state"] = output.state;
                input["response"] = match response {
                    Ok(value) => serde_json::json!({"data": value}),
                    Err(error) => serde_json::json!({"error": error}),
                };
                input["step"] = (step + 1).into();
                input["timestamp_ms"] = timestamp_ms().into();
            }
            _ => return Err("Invalid plugin screen or request chain".into()),
        }
    }
    Err("Plugin request limit reached".into())
}

#[cfg(target_os = "linux")]
pub fn run_installed(
    root: &std::path::Path,
    plugin: &crate::plugins::Plugin,
    allowed: bool,
    approved_origin: Option<String>,
    mut event: Event,
) -> Result<(Screen, Value), String> {
    use std::io::{Read, Write};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let code = crate::plugin_runtime::decode(
        plugin
            .wasm
            .as_deref()
            .ok_or("Plugin executable is unavailable")?,
    )?;
    let key = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(plugin.id.as_bytes())
    );
    let path = root.join(format!("extension-{key}.json"));
    if event.action == "open" {
        match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => {
                let metadata = file
                    .metadata()
                    .map_err(|_| "Could not read plugin settings")?;
                if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
                    return Err("Plugin settings require a private file".into());
                }
                let mut bytes = Vec::new();
                file.take(4097)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "Could not read plugin settings")?;
                if bytes.len() > 4096 {
                    return Err("Plugin settings exceed 4 KiB".into());
                }
                event.state =
                    serde_json::from_slice(&bytes).map_err(|_| "Invalid plugin settings")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Could not open plugin settings".into()),
        }
    }
    let hosts = if allowed {
        &plugin.https_hosts[..]
    } else {
        &[]
    };
    let (screen, state, save) = run(&code, event, |request| {
        let body = request.body.as_deref();
        if plugin.user_https_origin && request.host == "user" {
            let origin = approved_origin
                .as_deref()
                .ok_or("Approve the HTTPS endpoint first")?;
            let ca = root.join(format!("extension-{key}-ca.pem"));
            crate::download::approved_request(
                ca.is_file().then_some(ca.as_path()),
                origin,
                &request.path,
                &request.headers,
                body,
                if request.array.is_some() {
                    1_048_576
                } else {
                    24_576
                },
            )
        } else {
            let url = request.public_url(hosts)?;
            crate::download::public_request(
                Some(&root.join("https-roots.pem")),
                &url,
                &request.headers,
                body,
            )
        }
    })?;
    if save {
        let bytes = serde_json::to_vec(&state).map_err(|_| "Invalid plugin settings")?;
        let temporary = root.join(format!(
            ".extension-{key}-{}-{}.tmp",
            std::process::id(),
            timestamp_ms()
        ));
        let mut file = std::fs::OpenOptions::new()
            .mode(0o600)
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|_| "Could not stage plugin settings")?;
        if file
            .write_all(&bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| std::fs::rename(&temporary, &path))
            .is_err()
        {
            let _ = std::fs::remove_file(&temporary);
            return Err("Could not save plugin settings".into());
        }
    }
    Ok((screen, state))
}

fn timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

impl Request {
    fn validate_path(&self) -> Result<(), String> {
        if !self.path.starts_with('/')
            || self.path.starts_with("//")
            || self.path.len() > 1024
            || !self
                .path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-._~%?=&+".contains(&b))
            || self
                .path
                .split('?')
                .next()
                .unwrap_or_default()
                .split('/')
                .any(|part| part == "." || part == "..")
        {
            return Err("Invalid plugin HTTPS path".into());
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn public_url(&self, hosts: &[String]) -> Result<String, String> {
        if !hosts.contains(&self.host) || !valid_host(&self.host) {
            return Err("Plugin request exceeds its HTTPS permissions".into());
        }
        Ok(format!("https://{}{}", self.host, self.path))
    }
}

pub fn origin(url: &str) -> Option<(&str, u16)> {
    let authority = url.strip_prefix("https://")?;
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, port.parse::<u16>().ok()?),
        None => (authority, 443),
    };
    (port != 0 && (valid_host(host) || host.parse::<std::net::Ipv4Addr>().is_ok()))
        .then_some((host, port))
}

pub fn valid_host(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && host.parse::<std::net::Ipv4Addr>().is_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(action: &str, fields: Value, state: Value) -> Event {
        Event {
            action: action.into(),
            language: "en",
            fields: serde_json::from_value(fields).unwrap(),
            state,
        }
    }

    fn module(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("ToonShift-plugins/modules")
            .join(name);
        let code = std::fs::read(path).unwrap();
        crate::plugin_runtime::validate(&code, true).unwrap();
        code
    }

    #[test]
    fn screens_reject_unsafe_endpoints_and_paths() {
        assert_eq!(origin("https://ha.local:8123"), Some(("ha.local", 8123)));
        for url in [
            "http://ha.local",
            "https://ha.local/path",
            "https://ha.local@wrong.test",
            "https://ha.local:0",
            "https://ha.local#x",
        ] {
            assert!(origin(url).is_none(), "{url}");
        }
        for path in [
            "//wrong.test/",
            "/api/../bad",
            "/api/\r\n",
            "/api/#fragment",
        ] {
            let request: Request =
                serde_json::from_value(json!({"host":"test.foo","path":path})).unwrap();
            assert!(request.validate_path().is_err(), "{path}");
        }
    }

    #[test]
    #[ignore = "requires the separate ToonShift-plugins checkout"]
    fn tuya_module_owns_setup_discovery_command_and_readback() {
        let code = module("tuya-1.1.0.wasm");
        let (screen, _, save) = run(&code, event("open", json!({}), Value::Null), |_| {
            panic!("opening a screen must not use the network")
        })
        .unwrap();
        assert_eq!(screen.fields.len(), 4);
        assert!(screen
            .fields
            .iter()
            .any(|field| field.id == "secret" && field.secret));
        assert!(!save);
        let fields = json!({"region":"eu","client_id":"client","secret":"secret","uid":"user"});
        let execute = |request: &Request| {
            assert_eq!(request.host, "openapi.tuyaeu.com");
            assert_eq!(request.headers["sign"].len(), 64);
            let result = if request.path.starts_with("/v1.0/token?") {
                json!({"access_token":"testtoken","expire_time":7200})
            } else if request.path == "/v1.0/devices/plug/commands" {
                assert_eq!(
                    request.body.as_deref(),
                    Some(r#"{"commands":[{"code":"switch_1","value":true}]}"#)
                );
                json!(true)
            } else if request.path == "/v1.0/devices/plug/status" {
                json!([{"code":"switch_1","value":true}])
            } else {
                assert!(request.path.starts_with("/v1.0/users/user/devices?"));
                json!([{"id":"plug","name":"Desk","online":true,"status":[{"code":"switch_1","value":false}]}])
            };
            Ok(serde_json::to_vec(&json!({"success":true,"result":result})).unwrap())
        };
        let (screen, state, save) =
            run(&code, event("connect", fields, Value::Null), execute).unwrap();
        assert!(save);
        assert!(screen.buttons.iter().any(|button| button.id == "switch-0"));
        let mut paths = Vec::new();
        let (_, _, save) = run(&code, event("switch-0", json!({}), state), |request| {
            paths.push(request.path.clone());
            execute(request)
        })
        .unwrap();
        assert!(!save);
        assert_eq!(
            &paths[1..3],
            ["/v1.0/devices/plug/commands", "/v1.0/devices/plug/status"]
        );
    }

    #[test]
    #[ignore = "requires the separate ToonShift-plugins checkout"]
    fn home_assistant_module_uses_origin_grant_and_filters_switches() {
        let code = module("home-assistant-2.0.0.wasm");
        let (screen, _, _) = run(&code, event("open", json!({}), Value::Null), |_| {
            panic!("opening a screen must not use the network")
        })
        .unwrap();
        assert_eq!(screen.fields[0].id, "https_origin");
        let token = "fake-test-token-not-a-real-secret";
        let fields = json!({"https_origin":"https://ha.local:8123","token":token});
        let execute = |request: &Request| {
            assert_eq!(request.host, "user");
            assert_eq!(request.headers["Authorization"], format!("Bearer {token}"));
            let response = match request.path.as_str() {
                "/api/states" => {
                    assert!(request.array.is_some());
                    json!([
                        {"entity_id":"sensor.ignore","state":"22","attributes":{}},
                        {"entity_id":"switch.plug","state":"on","attributes":{"friendly_name":"Desk"}}
                    ])
                }
                "/api/services/switch/turn_off" => {
                    assert_eq!(
                        request.body.as_deref(),
                        Some(r#"{"entity_id":"switch.plug"}"#)
                    );
                    json!([])
                }
                "/api/states/switch.plug" => json!({"entity_id":"switch.plug","state":"off"}),
                path => panic!("Unexpected {path}"),
            };
            Ok(serde_json::to_vec(&response).unwrap())
        };
        let (screen, state, save) =
            run(&code, event("connect", fields, Value::Null), execute).unwrap();
        assert!(save);
        assert!(screen.fields[0].readonly);
        assert!(screen
            .buttons
            .iter()
            .any(|button| button.label.starts_with("Desk")));
        let mut paths = Vec::new();
        run(&code, event("switch-0", json!({}), state), |request| {
            paths.push(request.path.clone());
            execute(request)
        })
        .unwrap();
        assert_eq!(
            paths,
            [
                "/api/services/switch/turn_off",
                "/api/states/switch.plug",
                "/api/states"
            ]
        );
    }
}
