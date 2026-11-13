use std::io::{BufRead, BufReader, Read, Write};
use std::net::Ipv4Addr;
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::net::UnixDatagram;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct Join {
    pub name: String,
    pub password: String,
    pub encryption: String,
    pub auth: String,
    pub address: Option<StaticIpv4>,
}

pub struct StaticIpv4 {
    ip: Ipv4Addr,
    mask: Ipv4Addr,
    gateway: Ipv4Addr,
    dns: Ipv4Addr,
}

impl StaticIpv4 {
    pub fn parse(input: [&str; 4]) -> Option<Self> {
        let [ip, mask, gateway, dns] = input.map(|field| field.trim().parse::<Ipv4Addr>().ok());
        let (ip, mask, gateway, dns) = (ip?, mask?, gateway?, dns?);
        let bits = u32::from(mask);
        let prefix = bits.leading_ones();
        if !(1..=30).contains(&prefix) || bits != u32::MAX << (32 - prefix) {
            return None;
        }
        let network = u32::from(ip) & bits;
        let broadcast = network | !bits;
        let valid = |addr: Ipv4Addr| {
            !(addr.is_unspecified()
                || addr.is_loopback()
                || addr.is_multicast()
                || addr.is_broadcast())
        };
        if ![ip, gateway, dns].into_iter().all(valid)
            || u32::from(gateway) & bits != network
            || [ip, gateway]
                .into_iter()
                .any(|addr| [network, broadcast].contains(&u32::from(addr)))
            || ip == gateway
        {
            return None;
        }
        Some(Self {
            ip,
            mask,
            gateway,
            dns,
        })
    }

    fn xml(&self) -> String {
        format!("<type>static</type><ipaddress>{}</ipaddress><netmask>{}</netmask><default_gateway>{}</default_gateway><nameserver>{}</nameserver>", self.ip, self.mask, self.gateway, self.dns)
    }
}

pub struct Connection {
    pub ssid: String,
    pub address: Option<std::net::Ipv4Addr>,
    pub rssi: Option<i16>,
}

fn parse_connection(status: &str, signal: &str) -> Option<Connection> {
    let field = |name| {
        status
            .lines()
            .filter_map(|line| line.split_once('='))
            .find_map(|(key, value)| (key == name).then_some(value))
    };
    if field("wpa_state") != Some("COMPLETED") {
        return None;
    }
    let ssid = field("ssid")?
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    let address = field("ip_address")
        .and_then(|address| address.parse::<std::net::Ipv4Addr>().ok())
        .filter(|address| !address.is_unspecified());
    let rssi = signal
        .lines()
        .find_map(|line| line.strip_prefix("RSSI="))
        .and_then(|value| value.parse::<i16>().ok())
        .filter(|value| (-127..=0).contains(value));
    Some(Connection {
        ssid,
        address,
        rssi,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_static_ipv4_and_uses_firmware_field_names() {
        let address =
            StaticIpv4::parse(["192.168.1.20", "255.255.255.0", "192.168.1.1", "1.1.1.1"]).unwrap();
        assert_eq!(address.xml(), "<type>static</type><ipaddress>192.168.1.20</ipaddress><netmask>255.255.255.0</netmask><default_gateway>192.168.1.1</default_gateway><nameserver>1.1.1.1</nameserver>");
        assert!(
            StaticIpv4::parse(["192.168.1.20", "255.0.255.0", "192.168.1.1", "1.1.1.1"]).is_none()
        );
        assert!(
            StaticIpv4::parse(["192.168.1.20", "255.255.255.0", "192.168.2.1", "1.1.1.1"])
                .is_none()
        );
    }

    #[test]
    fn reports_association_without_an_address_and_filters_bad_signal() {
        assert!(parse_connection("wpa_state=SCANNING\nssid=Home", "RSSI=-40").is_none());
        let connection = parse_connection(
            "wpa_state=COMPLETED\nssid=Home=2\nip_address=0.0.0.0\n",
            "RSSI=999",
        )
        .unwrap();
        assert_eq!(connection.ssid, "Home=2");
        assert_eq!(connection.address, None);
        assert_eq!(connection.rssi, None);
        let connection = parse_connection(
            "wpa_state=COMPLETED\nssid=Home\nip_address=192.168.1.10\n",
            "RSSI=-57\n",
        )
        .unwrap();
        assert_eq!(
            connection.address,
            Some(std::net::Ipv4Addr::new(192, 168, 1, 10))
        );
        assert_eq!(connection.rssi, Some(-57));
    }
}

#[cfg(unix)]
pub fn connection() -> std::io::Result<Option<Connection>> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let path = format!(
        "/run/toonshift-wpa-info-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let socket = UnixDatagram::bind(&path)?;
    let result = (|| {
        socket.connect("/run/wpa_supplicant/wlan0")?;
        socket.set_read_timeout(Some(Duration::from_millis(500)))?;
        let query = |command: &[u8]| -> std::io::Result<String> {
            socket.send(command)?;
            let mut buffer = [0u8; 4096];
            let size = socket.recv(&mut buffer)?;
            Ok(String::from_utf8_lossy(&buffer[..size]).into_owned())
        };
        let status = query(b"STATUS")?;
        let signal = query(b"SIGNAL_POLL").unwrap_or_default();
        Ok(parse_connection(&status, &signal))
    })();
    let _ = std::fs::remove_file(path);
    result
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
    let address = match field("configtype") {
        "dhcp" => None,
        "static" => Some(
            StaticIpv4::parse([
                field("static_ipaddress"),
                field("static_netmask"),
                field("static_gateway"),
                field("static_nameserver"),
            ])
            .ok_or("Could not snapshot the existing static IPv4 configuration.")?,
        ),
        _ => return Err("Unsupported saved Wi-Fi address configuration.".into()),
    };
    let previous = Join {
        name: name.into(),
        password: field("wifi_key").into(),
        encryption: field("wifi_enc").into(),
        auth: field("wifi_auth").into(),
        address,
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
    if wait_for_connection(&join, deadline) {
        return Ok(());
    }
    if let Some((previous, bssid)) = previous {
        #[cfg(unix)]
        if matches!(send_join(&previous, &bssid), Ok(Ok(())))
            && wait_for_connection(&previous, Instant::now() + Duration::from_secs(15))
        {
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
pub fn reconnect() -> Result<(), String> {
    let (saved, bssid) = saved_network()?.ok_or("No saved Wi-Fi network.")?;
    // This firmware action resets the default-route interface, even when it is wired.
    let wifi_is_default = std::fs::read_to_string("/proc/net/route").is_ok_and(|routes| {
        let mut defaults = routes.lines().filter_map(|row| {
            let mut fields = row.split_whitespace();
            let interface = fields.next()?;
            (fields.next()? == "00000000").then_some(interface)
        });
        defaults.next() == Some("wlan0") && defaults.all(|interface| interface == "wlan0")
    });
    if wifi_is_default {
        crate::boxtalk::send_action(
            "hcb_netcon",
            "NetworkInformation",
            "ResetWirelessNetwork",
            "",
        )
        .map_err(|_| "Could not request a Wi-Fi reconnect.")?;
        std::thread::sleep(Duration::from_secs(2));
        if wait_for_connection(&saved, Instant::now() + Duration::from_secs(15)) {
            return Ok(());
        }
    }
    if matches!(send_join(&saved, &bssid), Ok(Ok(())))
        && wait_for_connection(&saved, Instant::now() + Duration::from_secs(15))
    {
        return Ok(());
    }
    Err("Could not reconnect to the saved Wi-Fi network.".into())
}

#[cfg(unix)]
fn wait_for_connection(join: &Join, deadline: Instant) -> bool {
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
                            .any(|line| line.strip_prefix("ssid=") == Some(&join.name));
                        let has_address = status
                            .lines()
                            .filter_map(|line| line.strip_prefix("ip_address="))
                            .filter_map(|value| value.parse::<Ipv4Addr>().ok())
                            .any(|address| {
                                !address.is_unspecified()
                                    && join
                                        .address
                                        .as_ref()
                                        .is_none_or(|expected| address == expected.ip)
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
    let address = join
        .address
        .as_ref()
        .map_or_else(|| "<type>dhcp</type>".into(), StaticIpv4::xml);
    write!(connection, "<action class=\"invoke\" uuid=\"{uuid}\" destuuid=\"{host}:hcb_netcon\" serviceid=\"urn:hcb-hae-com:serviceId:NetworkInformation\" requestid=\"{id}\"><u:SetWirelessNetworkInformation xmlns:u=\"urn:hcb-hae-com:service:NetworkInformation:1\"><iface>wlan0</iface>{address}<essid>{}</essid><enc>{}</enc><auth>{}</auth><key>{}</key>{bssid}</u:SetWirelessNetworkInformation></action>\0",
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
