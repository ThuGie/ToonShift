use crate::thermostat::{self, Command, Work};
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

const LISTEN: &str = "127.0.0.1:10082";

pub fn start(root: &Path, commands: Sender<Work>) -> io::Result<()> {
    let token = load_token(root)?;
    let listener = TcpListener::bind(LISTEN)?;
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            match connection {
                Ok(mut stream) => {
                    if let Err(error) = serve(&mut stream, &token, &commands) {
                        eprintln!("ToonShift API request failed: {error}");
                    }
                }
                Err(error) => eprintln!("ToonShift API connection failed: {error}"),
            }
        }
    });
    Ok(())
}

fn load_token(root: &Path) -> io::Result<String> {
    let path = root.join("api-token");
    if !path.exists() {
        let mut random = [0u8; 32];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
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

fn equal_secret(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0, |diff, (a, b)| diff | (a ^ b))
            == 0
}

fn serve(stream: &mut TcpStream, token: &str, commands: &Sender<Work>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let mut request = BufReader::new(stream.try_clone()?);
    let (status, response) = handle(&mut request, token, commands)?;
    let body = response.to_string();
    write!(stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len()
    )
}

fn handle(
    request: &mut impl BufRead,
    token: &str,
    commands: &Sender<Work>,
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
    if !authorization
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| equal_secret(value.as_bytes(), token.as_bytes()))
    {
        return Ok(("401 Unauthorized", json!({"error":"Bearer token required"})));
    }
    let mut body = vec![0; size.unwrap_or_default()];
    request.read_exact(&mut body)?;
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    match (method, route) {
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
        ("GET", "/v1/diagnostics") => Ok(read_result(thermostat::diagnostics())),
        ("GET", "/v1/energy") => {
            let mut meter = None;
            let mut period = None;
            for part in query.split('&') {
                let Some((key, value)) = part.split_once('=') else {
                    return Ok(bad_request());
                };
                match key {
                    "meter" if meter.is_none() => {
                        meter = serde_json::from_value::<crate::energy::Meter>(json!(value)).ok()
                    }
                    "period" if period.is_none() => {
                        period = serde_json::from_value::<crate::energy::Period>(json!(value)).ok()
                    }
                    _ => return Ok(bad_request()),
                }
            }
            Ok(match (meter, period) {
                (Some(meter), Some(period)) => read_result(crate::energy::load(meter, period)),
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
