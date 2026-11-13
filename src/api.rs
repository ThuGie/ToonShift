use crate::thermostat::{self, Command, Work};
use serde::Deserialize;
use serde_json::{json, Value};
#[cfg(target_os = "linux")]
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
#[cfg(target_os = "linux")]
use std::net::TcpListener;
use std::net::TcpStream;
#[cfg(target_os = "linux")]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
#[cfg(target_os = "linux")]
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
const LISTEN: &str = "127.0.0.1:10082";

#[derive(Default)]
pub struct Events {
    snapshot: Mutex<Snapshot>,
    changed: Condvar,
}

#[derive(Default)]
struct Snapshot {
    status_revision: u64,
    status: Option<Value>,
    readings_revision: u64,
    readings: crate::telemetry::Readings,
}

impl Events {
    pub fn publish_readings(&self, readings: &crate::telemetry::Readings) {
        let mut snapshot = self.snapshot.lock().unwrap();
        if snapshot.readings != *readings {
            snapshot.readings.clone_from(readings);
            snapshot.readings_revision += 1;
            self.changed.notify_all();
        }
    }

    fn readings(&self) -> Value {
        Self::format_readings(&self.snapshot.lock().unwrap().readings)
    }

    fn format_readings(values: &crate::telemetry::Readings) -> Value {
        use crate::plugins::Metric;
        let reading = |metric, unit, scale| {
            crate::telemetry::current(metric, values).map(|(value, age)| {
                json!({"value": value * scale, "unit": unit, "age_seconds": age.as_secs()})
            })
        };
        json!({
            "room_temperature": reading(Metric::RoomTemperature, "°C", 0.01),
            "room_humidity": reading(Metric::RoomHumidity, "% RH", 1.0),
            "electricity": reading(Metric::Electricity, "W", 1.0),
            "gas": reading(Metric::Gas, "L/h", 1.0),
        })
    }

    pub fn publish(&self, status: Option<&thermostat::Status>) {
        let payload = status.map(|status| json!(status));
        let mut snapshot = self.snapshot.lock().unwrap();
        if snapshot.status != payload {
            snapshot.status_revision += 1;
            snapshot.status = payload;
            self.changed.notify_all();
        }
    }
}

#[cfg(target_os = "linux")]
pub fn start(root: &Path, commands: Sender<Work>, events: Arc<Events>) -> io::Result<()> {
    let credentials = Arc::new(Credentials {
        control: load_token(root)?,
        clients: Some(root.join("api-clients")),
    });
    let listener = TcpListener::bind(LISTEN)?;
    let clients = Arc::new(Slots::new(12));
    let subscribers = Arc::new(Slots::new(4));
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            match connection {
                Ok(mut stream) => {
                    let Some(slot) = clients.take() else { continue };
                    let (credentials, commands, events, subscribers) = (
                        credentials.clone(),
                        commands.clone(),
                        events.clone(),
                        subscribers.clone(),
                    );
                    std::thread::spawn(move || {
                        let _slot = slot;
                        if let Err(error) =
                            serve(&mut stream, &credentials, &commands, &events, &subscribers)
                        {
                            eprintln!("ToonShift API request failed: {error}");
                        }
                    });
                }
                Err(error) => eprintln!("ToonShift API connection failed: {error}"),
            }
        }
    });
    Ok(())
}

struct Slots(std::sync::atomic::AtomicUsize);

impl Slots {
    fn new(count: usize) -> Self {
        Self(std::sync::atomic::AtomicUsize::new(count))
    }

    fn take(self: &Arc<Self>) -> Option<Slot> {
        use std::sync::atomic::Ordering::Relaxed;
        self.0
            .try_update(Relaxed, Relaxed, |count| count.checked_sub(1))
            .ok()?;
        Some(Slot(self.clone()))
    }
}

struct Slot(Arc<Slots>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0 .0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(target_os = "linux")]
fn load_token(root: &Path) -> io::Result<String> {
    let path = root.join("api-token");
    if !path.exists() {
        let token = random_token()?;
        let temporary = root.join("api-token.new");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
        fs::rename(temporary, &path)?;
    }
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::other("api-token must be a private regular file"));
    }
    let token = fs::read_to_string(path)?;
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other("Invalid API token"));
    }
    Ok(token)
}

#[cfg(target_os = "linux")]
fn random_token() -> io::Result<String> {
    let mut random = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn valid_client_id(id: &str) -> bool {
    (1..=40).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

#[cfg(target_os = "linux")]
pub fn manage_clients(root: &Path, mut args: impl Iterator<Item = String>) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let directory = root.join("api-clients");
    let command = args.next();
    let id = args.next();
    let scope = args.next();
    if args.next().is_some() {
        return Err(io::Error::other("Unexpected API client argument"));
    }
    match (command.as_deref(), id.as_deref(), scope.as_deref()) {
        (Some("create"), Some(id), Some(scope @ ("read" | "control"))) if valid_client_id(id) => {
            match fs::DirBuilder::new().mode(0o700).create(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            validate_client_directory(&directory)?;
            let token = random_token()?;
            let mut file = OpenOptions::new()
                .mode(0o600)
                .write(true)
                .create_new(true)
                .open(directory.join(id))?;
            if let Err(error) = writeln!(file, "{scope} {token}").and_then(|_| file.sync_all()) {
                let _ = fs::remove_file(directory.join(id));
                return Err(error);
            }
            println!("{id}.{token}");
        }
        (Some("list"), None, None) => {
            match validate_client_directory(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
            }
            let mut clients = Vec::new();
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                let name = entry.file_name();
                if let Some(id) = name.to_str().filter(|id| valid_client_id(id)) {
                    if let Some((grant, _)) = read_client(&entry.path()) {
                        clients.push(format!(
                            "{id} {}",
                            if grant == Grant::Read {
                                "read"
                            } else {
                                "control"
                            }
                        ));
                    }
                }
            }
            clients.sort();
            for client in clients {
                println!("{client}");
            }
        }
        (Some("revoke"), Some(id), None) if valid_client_id(id) => {
            validate_client_directory(&directory)?;
            fs::remove_file(directory.join(id))?;
        }
        _ => {
            return Err(io::Error::other(
                "Use --api-client create <id> <read|control>, list, or revoke <id>",
            ))
        }
    }
    Ok(())
}

fn validate_client_directory(directory: &std::path::Path) -> io::Result<()> {
    let info = std::fs::symlink_metadata(directory)?;
    #[cfg(unix)]
    let private = info.permissions().mode() & 0o077 == 0;
    #[cfg(not(unix))]
    let private = true;
    if !info.is_dir() || !private {
        return Err(io::Error::other("API client directory must be private"));
    }
    Ok(())
}

fn read_client(path: &std::path::Path) -> Option<(Grant, String)> {
    let info = std::fs::symlink_metadata(path).ok()?;
    if !info.is_file() || info.len() > 128 {
        return None;
    }
    #[cfg(unix)]
    if info.permissions().mode() & 0o077 != 0 {
        return None;
    }
    let data = std::fs::read_to_string(path).ok()?;
    let (scope, secret) = data.trim_end().split_once(' ')?;
    if secret.len() != 64 || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let grant = match scope {
        "read" => Grant::Read,
        "control" => Grant::Control,
        _ => return None,
    };
    Some((grant, secret.into()))
}

fn equal_secret(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

struct Credentials {
    control: String,
    clients: Option<std::path::PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Grant {
    Read,
    Control,
}

impl Credentials {
    fn grant(&self, token: &str) -> Option<Grant> {
        if equal_secret(token.as_bytes(), self.control.as_bytes()) {
            return Some(Grant::Control);
        }
        let (id, secret) = token.split_once('.')?;
        if !valid_client_id(id)
            || secret.len() != 64
            || !secret.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return None;
        }
        let directory = self.clients.as_ref()?;
        validate_client_directory(directory).ok()?;
        let (grant, expected) = read_client(&directory.join(id))?;
        equal_secret(secret.as_bytes(), expected.as_bytes()).then_some(grant)
    }
}

fn serve(
    stream: &mut TcpStream,
    credentials: &Credentials,
    commands: &Sender<Work>,
    events: &Events,
    subscribers: &Arc<Slots>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request = BufReader::new(stream.try_clone()?);
    let (status, response) = handle(&mut request, credentials, commands, events)?;
    if matches!(status, "event-stream" | "telemetry-stream") {
        if let Some(_slot) = subscribers.take() {
            let token = response
                .as_str()
                .ok_or_else(|| io::Error::other("Missing stream authorization"))?;
            return stream_events(stream, events, status == "telemetry-stream", || {
                credentials.grant(token).is_some()
            });
        }
        return send_json(
            stream,
            "503 Service Unavailable",
            json!({"error":"Event stream capacity reached"}),
        );
    }
    if status == "energy-csv" {
        let csv = response
            .as_str()
            .ok_or_else(|| io::Error::other("Invalid CSV response"))?;
        return write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/csv; charset=utf-8\r\nContent-Disposition: attachment; filename=toonshift-energy.csv\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{csv}", csv.len());
    }
    send_json(stream, status, response)
}

fn send_json(stream: &mut TcpStream, status: &str, response: Value) -> io::Result<()> {
    let body = response.to_string();
    write!(stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len()
    )
}

fn stream_events(
    stream: &mut impl Write,
    events: &Events,
    telemetry: bool,
    authorized: impl Fn() -> bool,
) -> io::Result<()> {
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\nretry: 2000\n\n")?;
    let until = Instant::now() + Duration::from_secs(300);
    let mut seen = None;
    let mut seen_readings = None;
    while Instant::now() < until {
        let snapshot = events.snapshot.lock().unwrap();
        let (snapshot, _) = events
            .changed
            .wait_timeout_while(snapshot, Duration::from_secs(10), |snapshot| {
                seen == Some(snapshot.status_revision)
                    && (!telemetry
                        || seen_readings
                            .is_some_and(|(revision, _)| revision == snapshot.readings_revision))
            })
            .unwrap();
        let revision = snapshot.status_revision;
        let status = (seen != Some(revision)).then(|| snapshot.status.clone());
        let readings = telemetry.then(|| {
            let values = Events::format_readings(&snapshot.readings);
            let available = ["room_temperature", "room_humidity", "electricity", "gas"]
                .map(|name| !values[name].is_null());
            ((snapshot.readings_revision, available), values)
        });
        drop(snapshot);
        if !authorized() {
            break;
        }
        let mut sent = false;
        if let Some(payload) = status {
            seen = Some(revision);
            match payload {
                Some(status) => write!(stream, "event: status\ndata: {status}\n\n")?,
                None => stream.write_all(b"event: unavailable\ndata: {}\n\n")?,
            }
            sent = true;
        }
        if let Some((key, values)) = readings {
            if seen_readings != Some(key) {
                seen_readings = Some(key);
                write!(stream, "event: telemetry\ndata: {values}\n\n")?;
                sent = true;
            }
        }
        if !sent {
            stream.write_all(b": keepalive\n\n")?;
        }
    }
    Ok(())
}

fn handle(
    request: &mut impl BufRead,
    credentials: &Credentials,
    commands: &Sender<Work>,
    events: &Events,
) -> io::Result<(&'static str, Value)> {
    let bad_request = || ("400 Bad Request", json!({"error":"Invalid HTTP request"}));
    let mut headers = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        let remaining = (4096 - headers.len()) as u64;
        let count = request.take(remaining).read_line(&mut line)?;
        if count == 0 || !line.ends_with("\r\n") {
            return Ok(bad_request());
        }
        if line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    let mut lines = headers.lines();
    let mut target = lines.next().unwrap_or_default().split_whitespace();
    let (method, path) = match (target.next(), target.next(), target.next(), target.next()) {
        (Some(method), Some(path), Some("HTTP/1.1" | "HTTP/1.0"), None) => (method, path),
        _ => return Ok(bad_request()),
    };
    let mut authorization = None;
    let mut size = None;
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            return Ok(bad_request());
        };
        if key.eq_ignore_ascii_case("authorization") {
            if authorization.replace(value.trim()).is_some() {
                return Ok(bad_request());
            }
        } else if key.eq_ignore_ascii_case("content-length") {
            let Ok(value) = value.trim().parse::<usize>() else {
                return Ok(bad_request());
            };
            if value > 256 || size.replace(value).is_some() {
                return Ok(bad_request());
            }
        } else if key.eq_ignore_ascii_case("transfer-encoding") {
            return Ok(bad_request());
        }
    }
    let token = authorization
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    let Some(grant) = credentials.grant(token) else {
        return Ok(("401 Unauthorized", json!({"error":"Bearer token required"})));
    };
    if grant == Grant::Read && method != "GET" {
        return Ok((
            "403 Forbidden",
            json!({"error":"Control permission required"}),
        ));
    }
    let mut body = vec![0; size.unwrap_or_default()];
    request.read_exact(&mut body)?;
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    match (method, route) {
        ("GET", "/v1/vacations") => Ok(read_result(crate::vacation::load())),
        ("POST" | "PUT", "/v1/vacations") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Dates { id: Option<String>, start: i32, end: i32 }
            let dates = serde_json::from_slice::<Dates>(&body).ok().and_then(|dates| {
                if dates.start <= 0 || dates.end <= dates.start { return None; }
                let id = match (method, dates.id.as_deref()) {
                    ("POST", None) => None,
                    ("PUT", Some(id)) => Some(crate::vacation::Id::parse(id)?),
                    _ => return None,
                };
                Some((id, dates.start, dates.end))
            });
            Ok(match dates {
                Some((id, start, end)) => submit(commands, Command::SaveVacation(id, start, end)),
                _ => ("422 Unprocessable Content", json!({"error":"Invalid vacation interval"})),
            })
        }
        ("DELETE", "/v1/vacations") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Selection { id: String }
            let id = serde_json::from_slice::<Selection>(&body).ok()
                .and_then(|selection| crate::vacation::Id::parse(&selection.id));
            Ok(match id {
                Some(id) => submit(commands, Command::RemoveVacation(id)),
                None => ("422 Unprocessable Content", json!({"error":"Invalid vacation ID"})),
            })
        }
        ("GET", "/v1/events") if query.is_empty() => Ok(("event-stream", json!(token))),
        ("GET", "/v1/events") if query == "telemetry=1" => Ok(("telemetry-stream", json!(token))),
        ("GET", "/v1/telemetry") if query.is_empty() => Ok(("200 OK", events.readings())),
        ("GET", "/v1/status") => Ok(match thermostat::status() {
            Ok(status) => ("200 OK", json!(status)),
            Err(_) => (
                "503 Service Unavailable",
                json!({"error":"Thermostat unavailable"}),
            ),
        }),
        ("GET", "/v1/program") if query == "revision=1" => Ok(read_result(crate::schedule::load().map(|slots| {
            json!({"revision":format!("{:016x}", crate::schedule::revision(&slots)), "periods":slots})
        }))),
        ("GET", "/v1/program") => Ok(read_result(crate::schedule::load())),
        ("PUT", "/v1/program") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct ProgramEdit { revision: String, edit: crate::schedule::Edit }
            let input = serde_json::from_slice::<ProgramEdit>(&body).ok().and_then(|input| {
                (input.revision.len() == 16).then_some(())?;
                Some((input.edit, u64::from_str_radix(&input.revision, 16).ok()?))
            });
            Ok(match input {
                Some((edit, revision)) => submit(commands, Command::EditScheduleAt(edit, revision)),
                None => ("422 Unprocessable Content", json!({"error":"Invalid schedule edit or revision"})),
            })
        }
        ("GET", "/v1/presets") => Ok(read_result(crate::presets::load())),
        ("PUT", "/v1/presets") => {
            let preset = serde_json::from_slice::<crate::presets::Preset>(&body)
                .ok()
                .filter(|preset| preset.id < 5 && (600..=3000).contains(&preset.temperature));
            Ok(match preset {
                Some(preset) => submit(commands, Command::SavePreset(preset)),
                None => ("422 Unprocessable Content", json!({"error":"Invalid preset configuration"})),
            })
        }
        ("GET", "/v1/capabilities") => {
            let boiler = crate::advanced::boiler().ok();
            let devices = crate::advanced::devices().ok();
            Ok(("200 OK", json!({
                "boiler": boiler.map(|boiler| json!({
                    "open_therm": boiler.open_therm,
                    "connected": boiler.connected,
                    "hot_water": boiler.hot_water.map(|water| json!({
                        "enabled": water.enabled,
                        "target": water.target,
                        "min": water.min,
                        "max": water.max,
                    })),
                })),
                "meters": devices.map(|devices| json!({
                    "configured_sources": devices.p1_sources,
                    "zwave_available": devices.zwave_available,
                    "adapters": devices.nodes.iter().filter(|node| node.meter).map(|node| json!({
                        "id": node.id,
                        "name": node.name,
                        "connected": node.connected,
                        "channels": node.channels.iter().map(|channel| channel.name).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                })),
            })))
        }
        ("GET", "/v1/diagnostics") => Ok(read_result(thermostat::diagnostics())),
        ("GET", "/v1/heating/history") => Ok(read_result(thermostat::history())),
        ("GET", "/v1/version") => Ok(("200 OK", json!({
            "toonshift": env!("CARGO_PKG_VERSION"),
            "firmware": thermostat::firmware_version().ok(),
        }))),
        ("GET", "/v1/energy" | "/v1/energy.csv") => {
            let mut meter = None;
            let mut period = None;
            for part in query.split('&') {
                let Some((key, value)) = part.split_once('=') else {
                    return Ok(bad_request());
                };
                match key {
                    "meter" if meter.is_none() => {
                        meter = serde_json::from_value::<crate::energy::Meter>(json!(value)).ok();
                        if meter.is_none() {
                            return Ok(bad_request());
                        }
                    }
                    "period" if period.is_none() => {
                        period = serde_json::from_value::<crate::energy::Period>(json!(value)).ok();
                        if period.is_none() {
                            return Ok(bad_request());
                        }
                    }
                    _ => return Ok(bad_request()),
                }
            }
            Ok(match (meter, period) {
                (Some(meter), Some(period)) => {
                    let result = crate::energy::load(meter, period);
                    match result {
                        Ok(chart) if route == "/v1/energy.csv" => ("energy-csv", json!(chart.csv())),
                        result => read_result(result),
                    }
                }
                _ => bad_request(),
            })
        }
        ("PUT", "/v1/preset") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Selection {
                id: u8,
            }
            Ok(
                match serde_json::from_slice::<Selection>(&body)
                    .ok()
                    .filter(|selection| selection.id < 5)
                {
                    Some(selection) => submit(commands, Command::ApplyPreset(selection.id)),
                    None => (
                        "422 Unprocessable Content",
                        json!({"error":"id must be an integer from 0 to 4"}),
                    ),
                },
            )
        }
        ("PUT", "/v1/hold") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Hold {
                preset: u8,
                mode: thermostat::HoldMode,
            }
            Ok(match serde_json::from_slice::<Hold>(&body) {
                Ok(hold) if hold.preset < 4 => submit(commands, Command::HoldPreset(hold.preset, hold.mode)),
                _ => ("422 Unprocessable Content", json!({"error":"Invalid hold mode or preset"})),
            })
        }
        ("PUT", "/v1/setpoint") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Setpoint {
                centi_celsius: i32,
            }
            let input = serde_json::from_slice::<Setpoint>(&body)
                .ok()
                .filter(|input| (600..=3000).contains(&input.centi_celsius));
            Ok(match input {
                Some(input) => submit(commands, Command::Setpoint(input.centi_celsius)),
                None => (
                    "422 Unprocessable Content",
                    json!({"error":"centi_celsius must be an integer from 600 to 3000"}),
                ),
            })
        }
        ("PUT", "/v1/schedule") => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Schedule {
                enabled: bool,
            }
            Ok(match serde_json::from_slice::<Schedule>(&body) {
                Ok(input) => submit(commands, Command::Program(input.enabled)),
                Err(_) => (
                    "422 Unprocessable Content",
                    json!({"error":"enabled must be a boolean"}),
                ),
            })
        }
        _ => Ok(("404 Not Found", json!({"error":"Unknown endpoint"}))),
    }
}

fn read_result<T: serde::Serialize>(result: Result<T, String>) -> (&'static str, Value) {
    match result {
        Ok(data) => ("200 OK", json!(data)),
        Err(_) => (
            "503 Service Unavailable",
            json!({"error":"Firmware data unavailable"}),
        ),
    }
}

fn submit(commands: &Sender<Work>, command: Command) -> (&'static str, Value) {
    let (reply, result) = mpsc::channel();
    if commands.send(Work::from_api(command, reply)).is_err() {
        return (
            "503 Service Unavailable",
            json!({"error":"Thermostat unavailable"}),
        );
    }
    match result.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => ("200 OK", json!({"result":"ok"})),
        Ok(Err(error)) if error == "Schedule revision conflict" => {
            ("409 Conflict", json!({"error":error}))
        }
        Ok(Err(error)) if matches!(command, Command::EditScheduleAt(_, _)) => {
            ("422 Unprocessable Content", json!({"error":error}))
        }
        Ok(Err(_)) => (
            "502 Bad Gateway",
            json!({"error":"Thermostat rejected the command"}),
        ),
        Err(_) => (
            "504 Gateway Timeout",
            json!({"error":"Command outcome unknown. Check status."}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn credentials() -> Credentials {
        Credentials {
            control: "test-token".into(),
            clients: None,
        }
    }

    #[test]
    fn read_client_cannot_write_and_can_be_revoked() {
        let path = std::env::temp_dir().join(format!("toonshift-api-test-{}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let secret = "ab".repeat(32);
        let file = path.join("monitor");
        std::fs::write(&file, format!("read {secret}")).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let credentials = Credentials {
            control: "test-token".into(),
            clients: Some(path.clone()),
        };
        let (commands, _) = mpsc::channel();
        let events = Events::default();
        let request = |method, route| {
            let request = format!(
                "{method} {route} HTTP/1.1\r\nAuthorization: Bearer monitor.{secret}\r\n\r\n"
            );
            handle(&mut request.as_bytes(), &credentials, &commands, &events)
                .unwrap()
                .0
        };
        assert_eq!(request("GET", "/v1/telemetry"), "200 OK");
        assert_eq!(request("PUT", "/v1/setpoint"), "403 Forbidden");
        std::fs::remove_file(file).unwrap();
        assert_eq!(request("GET", "/v1/telemetry"), "401 Unauthorized");
        std::fs::remove_dir(path).unwrap();
    }

    #[test]
    fn telemetry_keeps_zero_and_expires_stale_measurements() {
        use crate::plugins::Metric;
        let events = Events::default();
        let now = Instant::now();
        events.publish_readings(
            &[
                (Metric::RoomTemperature, (2140.0, now)),
                (Metric::Electricity, (0.0, now)),
                (Metric::RoomHumidity, (48.0, now - Duration::from_secs(901))),
            ]
            .into(),
        );
        let (commands, _) = mpsc::channel();
        let mut request =
            &b"GET /v1/telemetry HTTP/1.1\r\nAuthorization: Bearer test-token\r\n\r\n"[..];
        let (status, values) = handle(&mut request, &credentials(), &commands, &events).unwrap();
        assert_eq!(status, "200 OK");
        assert_eq!(values["electricity"]["value"], 0.0);
        assert!((values["room_temperature"]["value"].as_f64().unwrap() - 21.4).abs() < 1e-9);
        assert!(values["room_humidity"].is_null());
        assert!(values["gas"].is_null());
    }

    #[test]
    fn event_stream_requires_auth_and_delivers_current_state() {
        let credentials = credentials();
        let (commands, _) = mpsc::channel();
        let mut unauthorized = &b"GET /v1/events HTTP/1.1\r\n\r\n"[..];
        assert_eq!(
            handle(
                &mut unauthorized,
                &credentials,
                &commands,
                &Events::default()
            )
            .unwrap()
            .0,
            "401 Unauthorized"
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let events = Arc::new(Events::default());
        let producer = events.clone();
        let subscribers = Arc::new(Slots::new(1));
        std::thread::spawn(move || {
            let _ = serve(
                &mut server,
                &credentials,
                &commands,
                &producer,
                &subscribers,
            );
        });
        client
            .write_all(
                b"GET /v1/events?telemetry=1 HTTP/1.1\r\nAuthorization: Bearer test-token\r\n\r\n",
            )
            .unwrap();
        let mut reader = BufReader::new(client);
        let mut response = String::new();
        while !response.contains("event: unavailable\ndata: {}\n\n") {
            assert_ne!(reader.read_line(&mut response).unwrap(), 0);
        }
        assert!(response.contains("Content-Type: text/event-stream"));
        let status = serde_json::from_value(json!({
            "currentTemp":"2200", "currentSetpoint":"2000", "programState":"0",
            "burnerInfo":"0", "connection":"1", "nextSetpoint":"0", "nextTime":"0"
        }))
        .unwrap();
        events.publish(Some(&status));
        while !response.contains("\"currentTemp\":\"2200\"") {
            assert_ne!(reader.read_line(&mut response).unwrap(), 0);
        }
        assert!(response.contains("event: status\ndata: "));
        events.publish_readings(
            &[(crate::plugins::Metric::Electricity, (250.0, Instant::now()))].into(),
        );
        while !response.contains("\"value\":250.0") {
            assert_ne!(reader.read_line(&mut response).unwrap(), 0);
        }
        assert!(response.contains("event: telemetry\ndata: "));
    }
}
