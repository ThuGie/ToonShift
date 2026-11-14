use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn trust(root: &Path) -> io::Result<PathBuf> {
    let path = root.join("https-roots.pem");
    let temporary = root.join(format!("https-roots.{}.tmp", std::process::id()));
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(include_bytes!("../assets/isrg-root-x1.pem"))?;
    file.write_all(b"\n")?;
    file.write_all(include_bytes!("../assets/isrg-root-x2.pem"))?;
    std::fs::rename(temporary, &path)?;
    Ok(path)
}

pub fn get(ca: &Path, url: &str) -> Result<Vec<u8>, String> {
    fetch(ca, url, None, 32768)
}

pub const PACKAGE_LIMIT: usize = 196608;

pub fn package(ca: &Path, url: &str) -> Result<Vec<u8>, String> {
    fetch(ca, url, None, PACKAGE_LIMIT)
}

fn public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !matches!(a, 0 | 10 | 127 | 224..=255)
        && !(a == 100 && (64..=127).contains(&b)
            || a == 169 && b == 254
            || a == 172 && (16..=31).contains(&b)
            || a == 192 && matches!(b, 0 | 168)
            || a == 198 && matches!(b, 18 | 19)
            || a == 198 && b == 51 && c == 100
            || a == 203 && b == 0 && c == 113)
}

pub fn public_json(ca: &Path, url: &str) -> Result<Vec<u8>, String> {
    public_request(Some(ca), url, &BTreeMap::new(), None)
}

pub fn public_request(
    ca: Option<&Path>,
    url: &str,
    headers: &BTreeMap<String, String>,
    body: Option<&str>,
) -> Result<Vec<u8>, String> {
    let authority = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split_once('/'))
        .map(|(host, _)| host)
        .ok_or("Plugin feed must use HTTPS and a path")?;
    if authority.len() > 253
        || !authority.contains('.')
        || url.contains(['\r', '\n', '\\', '\0'])
        || authority.parse::<Ipv4Addr>().is_ok()
        || !authority.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err("Plugin feed requires a public DNS hostname".into());
    }
    let endpoint = (authority, 443)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| {
            addresses.find_map(|endpoint| match endpoint.ip() {
                std::net::IpAddr::V4(ip) if public_ipv4(ip) => Some(ip),
                _ => None,
            })
        })
        .or_else(|| firmware_dns(authority))
        .ok_or("Plugin feeds require a public IPv4 destination")?;
    let resolve = format!("{authority}:443:{endpoint}");
    transfer(ca, url, Some(&resolve), 32768, headers, body)
}

#[cfg(target_os = "linux")]
pub fn approved_request(
    ca: Option<&Path>,
    origin: &str,
    path: &str,
    headers: &BTreeMap<String, String>,
    body: Option<&str>,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let (host, port) =
        crate::plugin_screen::origin(origin).ok_or("Invalid approved HTTPS endpoint")?;
    let endpoint = (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut endpoints| {
            endpoints.find_map(|endpoint| match endpoint.ip() {
                std::net::IpAddr::V4(ip) if public_ipv4(ip) || ip.is_private() => Some(ip),
                _ => None,
            })
        })
        .ok_or("HTTPS endpoint requires a LAN or public IPv4 address")?;
    let resolve = format!("{host}:{port}:{endpoint}");
    transfer(
        ca,
        &format!("{origin}{path}"),
        Some(&resolve),
        limit,
        headers,
        body,
    )
}

fn firmware_dns(host: &str) -> Option<Ipv4Addr> {
    use std::time::{Duration, Instant};
    let mut child = Command::new("/usr/bin/nslookup")
        .arg(host)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut output = String::new();
    child
        .stdout
        .take()?
        .take(8192)
        .read_to_string(&mut output)
        .ok()?;
    let (_, answer) = output.split_once("\nName:")?;
    answer
        .lines()
        .filter_map(|line| {
            let (label, value) = line.split_once(':')?;
            if !label.trim().starts_with("Address") {
                return None;
            }
            value.split_whitespace().next()?.parse::<Ipv4Addr>().ok()
        })
        .find(|ip| public_ipv4(*ip))
}

fn fetch(ca: &Path, url: &str, resolve: Option<&str>, limit: usize) -> Result<Vec<u8>, String> {
    transfer(Some(ca), url, resolve, limit, &BTreeMap::new(), None)
}

fn transfer(
    ca: Option<&Path>,
    url: &str,
    resolve: Option<&str>,
    limit: usize,
    headers: &BTreeMap<String, String>,
    body: Option<&str>,
) -> Result<Vec<u8>, String> {
    let mut curl = Command::new("/usr/bin/curl");
    curl.args([
        "-q",
        "-fsS",
        "--globoff",
        "--proto",
        "=https",
        "--connect-timeout",
        "5",
        "--max-time",
        "10",
        "--max-filesize",
        &limit.to_string(),
    ]);
    if let Some(ca) = ca {
        curl.arg("--cacert").arg(ca);
    }
    if let Some(resolve) = resolve {
        curl.args(["--noproxy", "*", "--resolve", resolve]);
    }
    let input = request_config(url, headers, body)?;
    let mut child = curl
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let write = child.stdin.take().unwrap().write_all(input.as_bytes());
    let mut response = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take(limit as u64 + 1)
        .read_to_end(&mut response);
    if write.is_err() || read.is_err() || response.len() > limit {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    write.map_err(|error| error.to_string())?;
    read.map_err(|error| error.to_string())?;
    if !status.success() || response.len() > limit {
        return Err("HTTPS request failed".into());
    }
    Ok(response)
}

fn request_config(
    url: &str,
    headers: &BTreeMap<String, String>,
    body: Option<&str>,
) -> Result<String, String> {
    // Keep signed request headers and payloads out of the process argument list.
    let quoted = |value: &str| -> Result<String, String> {
        if value.chars().any(char::is_control) {
            return Err("Invalid HTTP request argument".into());
        }
        Ok(format!(
            "\"{}\"",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        ))
    };
    if headers.len() > 16 || body.is_some_and(|body| body.len() > 4096) {
        return Err("HTTP request exceeds the plugin limits".into());
    }
    let mut config = format!("url = {}\n", quoted(url)?);
    for (name, value) in headers {
        if name.is_empty()
            || name.len() > 64
            || value.len() > 2048
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
            || matches!(
                name.to_ascii_lowercase().as_str(),
                "host"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "content-type"
                    | "expect"
                    | "upgrade"
            )
            || name.to_ascii_lowercase().starts_with("proxy-")
        {
            return Err("Invalid plugin HTTP header".into());
        }
        config.push_str(&format!(
            "header = {}\n",
            quoted(&format!("{name}: {value}"))?
        ));
    }
    if let Some(body) = body {
        if !matches!(body.as_bytes().first(), Some(b'{' | b'[')) {
            return Err("Plugin request body must be a JSON object or array".into());
        }
        config.push_str("header = \"Content-Type: application/json\"\n");
        config.push_str(&format!("data-binary = {}\n", quoted(body)?));
    }
    Ok(config)
}

pub fn query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to string");
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    #[test]
    fn plugin_feeds_exclude_local_and_shared_ipv4_ranges() {
        for ip in [
            "127.0.0.1",
            "192.168.1.1",
            "172.16.2.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.169.254",
            "224.0.0.1",
        ] {
            assert!(!super::public_ipv4(ip.parse().unwrap()), "{ip}");
        }
        assert!(super::public_ipv4("1.1.1.1".parse().unwrap()));
    }
}
