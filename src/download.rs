use std::fmt::Write as _;
use std::io::{self, Read, Write};
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
    let mut child = Command::new("/usr/bin/curl")
        .args([
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
            "32768",
            "--cacert",
        ])
        .arg(ca)
        .arg("--")
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take(32769)
        .read_to_end(&mut response);
    let status = child.wait().map_err(|error| error.to_string())?;
    read.map_err(|error| error.to_string())?;
    if !status.success() || response.len() > 32768 {
        return Err("HTTPS request failed".into());
    }
    Ok(response)
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
