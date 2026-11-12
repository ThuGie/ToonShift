use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub fn thermostat(verbs: &[&str]) -> io::Result<Vec<String>> {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")?;
    let host = host.trim();
    if host.is_empty() || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(io::Error::other("Invalid hostname"));
    }
    let id = format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let uuid = format!("{host}:toonshift-{id}");
    let mut socket = TcpStream::connect("127.0.0.1:1337")?;
    socket.set_write_timeout(Some(Duration::from_secs(2)))?;
    for state in ["connect", "alive"] {
        write!(socket, "<discovery nts=\"ssdp:{state}\" uuid=\"{uuid}\" type=\"urn:schemas-hcb-hae-com:device:toonshift\" version=\"v\" sessionKey=\"{id}\"></discovery>\0")?;
    }
    for (index, verb) in verbs.iter().enumerate() {
        write!(socket, "<action class=\"invoke\" uuid=\"{uuid}\" destuuid=\"{host}:happ_thermstat\" serviceid=\"urn:hcb-hae-com:serviceId:Thermostat\" requestid=\"{id}-{index}\"><u:{verb} xmlns:u=\"urn:hcb-hae-com:service:Thermostat:1\"></u:{verb}></action>\0")?;
    }
    let mut reader = BufReader::new(socket);
    let mut frame = Vec::new();
    let mut replies = vec![None; verbs.len()];
    let deadline = Instant::now() + Duration::from_secs(3);
    while replies.iter().any(Option::is_none) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::other("BoxTalk response timeout"));
        }
        reader.get_ref().set_read_timeout(Some(remaining))?;
        frame.clear();
        let size = reader.by_ref().take(65536).read_until(0, &mut frame)?;
        if size == 0 || frame.last() != Some(&0) {
            return Err(io::Error::other("Incomplete BoxTalk frame"));
        }
        let Ok(xml) = std::str::from_utf8(&frame[..size - 1]) else {
            continue;
        };
        if !xml.contains("class=\"response\"")
            || !xml.contains(&format!("uuid=\"{host}:happ_thermstat\""))
        {
            continue;
        }
        for (index, reply) in replies.iter_mut().enumerate() {
            if xml.contains(&format!("requestid=\"{id}-{index}\"")) {
                *reply = Some(xml.to_owned());
                break;
            }
        }
    }
    Ok(replies.into_iter().flatten().collect())
}

pub fn field<'a>(xml: &'a str, key: &str) -> Option<&'a str> {
    let (_, tail) = xml.split_once(&format!("<{key}>"))?;
    tail.split_once(&format!("</{key}>"))
        .map(|(value, _)| value.trim())
}
