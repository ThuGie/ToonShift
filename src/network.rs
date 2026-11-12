use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::net::UnixDatagram;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct Join {
    pub name: String,
    pub password: String,
    pub encryption: String,
    pub auth: String,
}

impl Join {
    fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() || self.name.len() > 32 || self.name.chars().any(char::is_control) {
            return Err("Enter a Wi-Fi network name of 1–32 bytes.".into());
        }
        if !matches!(self.auth.as_str(), "WPA" | "WPA2" | "OPEN" | "NONE") {
            return Err("This network security mode is unavailable on this Toon.".into());
        }
        if matches!(self.auth.as_str(), "WPA" | "WPA2")
            && !(self.password.is_ascii()
                && ((8..=63).contains(&self.password.len())
                    || (self.password.len() == 64
                        && self.password.bytes().all(|b| b.is_ascii_hexdigit()))))
        {
            return Err("Enter an 8–63 character Wi-Fi password or a 64-digit hex key.".into());
        }
        Ok(())
    }
}

fn xml(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn saved_network() -> Result<Option<(Join, String)>, String> {
    let response = crate::thermostat::firmware_get(
        "hcb_config",
        "action=getObjectConfig&package=hcb_netcon&internalAddress=iface-wlan0",
    )
    .map_err(|_| "Could not read the current Wi-Fi configuration.")?;
    let config: serde_json::Value = serde_json::from_str(&response)
        .map_err(|_| "Could not read the current Wi-Fi configuration.")?;
    let settings = config["device"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find_map(|item| item["interfaceSettings"].as_array())
        })
        .ok_or("Could not read the current Wi-Fi configuration.")?;
    let field = |name: &str| {
        settings
            .iter()
            .find_map(|item| item[name].as_str())
            .unwrap_or_default()
    };
    let name = field("wifi_essid");
    if name.is_empty() || field("disabled") == "1" {
        return Ok(None);
    }
    if field("configtype") != "dhcp" {
        return Err("Use stock setup to replace an existing static Wi-Fi configuration.".into());
    }
    let previous = Join {
        name: name.into(),
        password: field("wifi_key").into(),
        encryption: field("wifi_enc").into(),
        auth: field("wifi_auth").into(),
    };
    previous
        .validate()
        .map_err(|_| "The existing Wi-Fi settings cannot be restored by ToonShift.")?;
    Ok(Some((previous, field("wifi_bssid").into())))
}

pub fn join(join: Join) -> Result<(), String> {
    join.validate()?;
    let previous = saved_network()?;
    let deadline = Instant::now() + Duration::from_secs(45);
    let applied = send_join(&join, "");
    if let Ok(Err(error)) = applied {
        return Err(error);
    }
    #[cfg(unix)]
    if wait_for_connection(&join.name, deadline) {
        return Ok(());
    }
    if let Some((previous, bssid)) = previous {
        if matches!(send_join(&previous, &bssid), Ok(Ok(()))) {
            return Err("Connection failed. Previous Wi-Fi settings restored.".into());
        }
        return Err(
            "Connection failed and restoration was not confirmed. Open stock setup to recover."
                .into(),
        );
    }
    Err("The Toon could not confirm the Wi-Fi connection.".into())
}

#[cfg(unix)]
fn wait_for_connection(name: &str, deadline: Instant) -> bool {
    let path = format!("/run/toonshift-wpa-{}", std::process::id());
    let _ = std::fs::remove_file(&path);
    let Ok(socket) = UnixDatagram::bind(&path) else {
        return false;
    };
    let result = (|| {
        socket
            .set_read_timeout(Some(Duration::from_millis(500)))
            .ok()?;
        let mut reply = [0u8; 4096];
        while Instant::now() < deadline {
            if socket
                .send_to(b"STATUS", "/run/wpa_supplicant/wlan0")
                .is_ok()
            {
                if let Ok(count) = socket.recv(&mut reply) {
                    if let Ok(status) = std::str::from_utf8(&reply[..count]) {
                        let connected = status.lines().any(|line| line == "wpa_state=COMPLETED");
                        let selected = status
                            .lines()
                            .any(|line| line.strip_prefix("ssid=") == Some(name));
                        let has_address = status.lines().any(|line| {
                            line.starts_with("ip_address=") && line != "ip_address=0.0.0.0"
                        });
                        if connected && selected && has_address {
                            return Some(());
                        }
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        None
    })();
    let _ = std::fs::remove_file(path);
    result.is_some()
}

fn send_join(join: &Join, bssid: &str) -> std::io::Result<Result<(), String>> {
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")?;
    let host = host.trim();
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(std::io::Error::other("Invalid hostname"));
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let id = format!("{}-{now}", std::process::id());
    let uuid = format!("{host}:toonshift-wifi");
    let mut connection = TcpStream::connect("127.0.0.1:1337")?;
    connection.set_write_timeout(Some(Duration::from_secs(2)))?;
    for event in ["connect", "alive"] {
        write!(connection, "<discovery nts=\"ssdp:{event}\" uuid=\"{uuid}\" type=\"urn:schemas-hcb-hae-com:device:toonshift\" version=\"v\" sessionKey=\"{id}\"></discovery>\0")?;
    }
    let bssid = if bssid.is_empty() || bssid == "00:00:00:00:00:00" {
        String::new()
    } else {
        format!("<bssid>{}</bssid>", xml(bssid))
    };
    write!(connection, "<action class=\"invoke\" uuid=\"{uuid}\" destuuid=\"{host}:hcb_netcon\" serviceid=\"urn:hcb-hae-com:serviceId:NetworkInformation\" requestid=\"{id}\"><u:SetWirelessNetworkInformation xmlns:u=\"urn:hcb-hae-com:service:NetworkInformation:1\"><iface>wlan0</iface><type>dhcp</type><essid>{}</essid><enc>{}</enc><auth>{}</auth><key>{}</key>{bssid}</u:SetWirelessNetworkInformation></action>\0",
        xml(&join.name), xml(&join.encryption), xml(&join.auth), xml(&join.password))?;
    let mut reader = BufReader::new(connection);
    let mut frame = Vec::new();
    let expected = format!("requestid=\"{id}\"");
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        reader.get_ref().set_read_timeout(Some(
            deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1)),
        ))?;
        frame.clear();
        let size = reader.by_ref().take(65536).read_until(0, &mut frame)?;
        if size == 0 || frame.last() != Some(&0) {
            break;
        }
        let Ok(message) = std::str::from_utf8(&frame) else {
            continue;
        };
        if message.contains(&expected) && message.contains("class=\"response\"") {
            return Ok(
                if message.contains("<result>ok</result>")
                    || message.contains("<result>no change</result>")
                {
                    Ok(())
                } else {
                    Err("The Toon network service rejected the settings.".into())
                },
            );
        }
    }
    Err(std::io::Error::other("Missing Wi-Fi response"))
}
