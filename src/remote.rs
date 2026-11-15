use crate::{api, thermostat::Work};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{self, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{mpsc::Sender, Arc, Mutex};
use std::time::{Duration, Instant};

const PORT: u16 = 10083;
type Reply = (&'static str, Value);

#[derive(Clone, Serialize, Deserialize)]
pub struct Phone {
    pub id: String,
    pub name: String,
    verifier: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Identity {
    address: Ipv4Addr,
    cert: Vec<u8>,
    key: Vec<u8>,
    phones: Vec<Phone>,
}

struct Pending {
    secret: String,
    id: String,
    expires: Instant,
    attempts: u8,
    phone: Option<(Phone, String)>,
}

#[derive(Default)]
struct State {
    enabled: bool,
    listening: bool,
    generation: u64,
    identity: Option<Identity>,
    pairing: Option<Pending>,
}

pub struct Remote {
    root: PathBuf,
    state: Mutex<State>,
}

fn private_read<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o077 != 0 || metadata.len() > 65536 {
        return Err(io::Error::other("Invalid private remote settings"));
    }
    serde_json::from_slice(&std::fs::read(path)?).map_err(io::Error::other)
}

fn private_write(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let temp = path.with_extension("new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .mode(0o600)
        .open(&temp)?;
    serde_json::to_writer(&mut file, value)?;
    file.sync_all()?;
    std::fs::rename(temp, path)?;
    std::fs::File::open(path.parent().unwrap())?.sync_all()
}

impl Remote {
    pub fn start(
        root: &Path,
        commands: Sender<Work>,
        events: Arc<api::Events>,
        enabled: bool,
    ) -> Arc<Self> {
        let identity = private_read(&root.join("remote-identity.json")).ok();
        let remote = Arc::new(Self {
            root: root.into(),
            state: Mutex::new(State {
                identity,
                enabled,
                ..State::default()
            }),
        });
        let service = remote.clone();
        std::thread::spawn(move || service.listen(commands, events));
        remote
    }

    pub fn configure(&self, address: Ipv4Addr) -> Result<(), String> {
        if !address.is_private() {
            return Err("Remote access requires a private LAN IPv4 address".into());
        }
        let mut state = self.state.lock().unwrap();
        if state
            .identity
            .as_ref()
            .is_some_and(|identity| identity.address == address)
        {
            return Ok(());
        }
        let cert = rcgen::generate_simple_self_signed(vec![address.to_string()])
            .map_err(|e| e.to_string())?;
        let identity = Identity {
            address,
            cert: cert.cert.der().to_vec(),
            key: cert.key_pair.serialize_der(),
            phones: Vec::new(),
        };
        self.save(&identity)?;
        state.identity = Some(identity);
        state.generation = state.generation.wrapping_add(1);
        state.listening = false;
        state.pairing = None;
        Ok(())
    }

    fn save(&self, identity: &Identity) -> Result<(), String> {
        private_write(&self.root.join("remote-identity.json"), identity).map_err(|e| e.to_string())
    }

    pub fn enable(&self, enabled: bool) {
        let mut state = self.state.lock().unwrap();
        if state.enabled != enabled {
            state.enabled = enabled;
            state.listening = false;
            state.pairing = None;
            state.generation = state.generation.wrapping_add(1);
        }
    }

    pub fn pair(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if !state.enabled || !state.listening {
            return Err("Remote HTTPS is not ready".into());
        }
        if state
            .identity
            .as_ref()
            .is_some_and(|id| id.phones.len() >= 8)
        {
            return Err("Remove a paired phone first".into());
        }
        state.pairing = Some(Pending {
            secret: api::random_token().map_err(|e| e.to_string())?,
            id: api::random_token().map_err(|e| e.to_string())?[..16].to_owned(),
            expires: Instant::now() + Duration::from_secs(120),
            attempts: 0,
            phone: None,
        });
        Ok(())
    }

    pub fn cancel(&self) {
        self.state.lock().unwrap().pairing = None;
    }

    pub fn approve(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let phone = state
            .pairing
            .as_ref()
            .filter(|p| p.expires > Instant::now())
            .and_then(|p| p.phone.as_ref())
            .map(|p| p.0.clone())
            .ok_or("Pairing expired")?;
        let mut identity = state
            .identity
            .clone()
            .ok_or("Remote identity unavailable")?;
        identity.phones.push(phone);
        self.save(&identity)?;
        state.identity = Some(identity);
        state.pairing = None;
        Ok(())
    }

    pub fn revoke(&self, id: &str) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let mut identity = state
            .identity
            .clone()
            .ok_or("Remote identity unavailable")?;
        identity.phones.retain(|phone| phone.id != id);
        self.save(&identity)?;
        state.identity = Some(identity);
        Ok(())
    }

    pub fn view(&self) -> crate::ui::RemoteView {
        let state = self.state.lock().unwrap();
        let mut view = crate::ui::RemoteView {
            enabled: state.enabled,
            listening: state.listening,
            addresses: addresses(),
            ..Default::default()
        };
        if let Some(identity) = &state.identity {
            view.phones = identity
                .phones
                .iter()
                .map(|phone| (phone.id.clone(), phone.name.clone()))
                .collect();
            view.address = Some(identity.address);
            if let Some(pair) = state
                .pairing
                .as_ref()
                .filter(|p| p.expires > Instant::now())
            {
                if let Some((phone, code)) = &pair.phone {
                    view.approval = Some((phone.name.clone(), code.clone()));
                } else {
                    view.qr = Some(json!({"v":1,"host":identity.address.to_string(),"pin":format!("{:x}", Sha256::digest(&identity.cert)),
                        "secret":pair.secret,"id":pair.id}).to_string());
                }
            }
        }
        view
    }

    fn claim(&self, generation: u64, body: &[u8]) -> Reply {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Claim {
            secret: String,
            name: String,
            verifier: String,
        }
        let mut state = self.state.lock().unwrap();
        if !state.enabled || state.generation != generation {
            return unauthorized();
        }
        let Some(pair) = state.pairing.as_mut().filter(|pair| {
            pair.expires > Instant::now() && pair.attempts < 5 && pair.phone.is_none()
        }) else {
            return ("410 Gone", json!({"error":"Pairing closed"}));
        };
        pair.attempts += 1;
        let Ok(claim) = serde_json::from_slice::<Claim>(body) else {
            return unauthorized();
        };
        if !api::equal_secret(claim.secret.as_bytes(), pair.secret.as_bytes())
            || !valid_secret(&claim.verifier)
            || claim.name.trim().is_empty()
            || claim.name.len() > 32
            || claim.name.chars().any(char::is_control)
        {
            return unauthorized();
        }
        let code = u32::from_str_radix(&claim.verifier[..8], 16).unwrap() % 1_000_000;
        let code = format!("{code:06}");
        pair.phone = Some((
            Phone {
                id: pair.id.clone(),
                name: claim.name,
                verifier: claim.verifier,
            },
            code.clone(),
        ));
        ("202 Accepted", json!({"code":code}))
    }

    fn authorize(&self, generation: u64, token: &str) -> Option<api::Grant> {
        let (id, secret) = token.split_once('.')?;
        if !valid_secret(secret) {
            return None;
        }
        let state = self.state.lock().unwrap();
        if !state.enabled || state.generation != generation {
            return None;
        }
        let phone = state
            .identity
            .as_ref()?
            .phones
            .iter()
            .find(|phone| phone.id == id)?;
        let hash = format!("{:x}", Sha256::digest(secret.as_bytes()));
        api::equal_secret(hash.as_bytes(), phone.verifier.as_bytes()).then_some(api::Grant::Control)
    }

    fn listen(self: Arc<Self>, commands: Sender<Work>, events: Arc<api::Events>) {
        let clients = Arc::new(api::Slots::new(8));
        let streams = Arc::new(api::Slots::new(3));
        let mut active: Option<(u64, TcpListener, Arc<rustls::ServerConfig>)> = None;
        loop {
            let next = {
                let state = self.state.lock().unwrap();
                (state.enabled
                    && active
                        .as_ref()
                        .is_none_or(|(generation, _, _)| *generation != state.generation))
                .then(|| (state.generation, state.identity.clone()))
                .filter(|(_, id)| id.is_some())
            };
            if let Some((generation, Some(identity))) = next {
                active = None;
                if let Ok((listener, tls)) = bind(identity) {
                    let mut state = self.state.lock().unwrap();
                    if state.enabled && state.generation == generation {
                        state.listening = true;
                        active = Some((generation, listener, tls));
                    }
                }
            }
            let Some((generation, listener, tls)) = &active else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            if !self.current(*generation) {
                active = None;
                continue;
            }
            match listener.accept() {
                Ok((socket, _)) => {
                    let Some(slot) = clients.take() else { continue };
                    let (service, commands, events, tls, streams, generation) = (
                        self.clone(),
                        commands.clone(),
                        events.clone(),
                        tls.clone(),
                        streams.clone(),
                        *generation,
                    );
                    std::thread::spawn(move || {
                        let _slot = slot;
                        let _ =
                            service.serve(socket, tls, generation, &commands, &events, &streams);
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(200))
                }
                Err(_) => {
                    self.state.lock().unwrap().listening = false;
                    active = None;
                }
            }
        }
    }

    fn current(&self, generation: u64) -> bool {
        let state = self.state.lock().unwrap();
        state.enabled && state.generation == generation
    }

    fn serve(
        &self,
        socket: TcpStream,
        tls: Arc<rustls::ServerConfig>,
        generation: u64,
        commands: &Sender<Work>,
        events: &api::Events,
        streams: &Arc<api::Slots>,
    ) -> io::Result<()> {
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        socket.set_write_timeout(Some(Duration::from_secs(3)))?;
        let connection = rustls::ServerConnection::new(tls).map_err(io::Error::other)?;
        let mut stream = rustls::StreamOwned::new(connection, socket);
        let (status, response) = api::handle_with(
            &mut BufReader::new(&mut stream),
            |token| self.authorize(generation, token),
            |method, path, token, body| {
                if !self.current(generation) {
                    return Some(unauthorized());
                }
                match (method, path) {
                    ("POST", "/v1/pair/claim") => Some(self.claim(generation, body)),
                    ("GET", "/v1/device") | ("DELETE", "/v1/device") => {
                        if self.authorize(generation, token).is_none() {
                            return Some(unauthorized());
                        }
                        let (id, _) = token.split_once('.').unwrap();
                        if method == "DELETE" && self.revoke(id).is_err() {
                            return Some((
                                "503 Service Unavailable",
                                json!({"error":"Could not revoke phone"}),
                            ));
                        }
                        Some((
                            "200 OK",
                            json!({"name":"ToonShift","id":id,"timezone":"Europe/Amsterdam"}),
                        ))
                    }
                    _ if allowed(method, path) => None,
                    _ => Some(("404 Not Found", json!({"error":"Unknown endpoint"}))),
                }
            },
            commands,
            events,
        )?;
        if status == "event-stream" {
            if let Some(_slot) = streams.take() {
                let token = response.as_str().unwrap_or_default();
                return api::stream_events(&mut stream, events, false, || {
                    self.authorize(generation, token).is_some()
                });
            }
            return api::send_json(
                &mut stream,
                "503 Service Unavailable",
                json!({"error":"Status streams busy"}),
            );
        }
        api::send_json(&mut stream, status, response)?;
        stream.flush()
    }
}

fn addresses() -> Vec<Ipv4Addr> {
    let mut first = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        return Vec::new();
    }
    let mut values = Vec::new();
    let mut current = first;
    while let Some(interface) = unsafe { current.as_ref() } {
        if !interface.ifa_addr.is_null()
            && unsafe { (*interface.ifa_addr).sa_family as i32 } == libc::AF_INET
        {
            let socket = unsafe { &*(interface.ifa_addr.cast::<libc::sockaddr_in>()) };
            let address = Ipv4Addr::from(socket.sin_addr.s_addr.to_ne_bytes());
            if address.is_private()
                && interface.ifa_flags & libc::IFF_UP as u32 != 0
                && !values.contains(&address)
            {
                values.push(address);
            }
        }
        current = interface.ifa_next;
    }
    unsafe { libc::freeifaddrs(first) };
    values
}

fn bind(
    identity: Identity,
) -> Result<(TcpListener, Arc<rustls::ServerConfig>), Box<dyn std::error::Error>> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    let tls = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from(identity.cert)],
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key)),
    )?;
    let listener = TcpListener::bind((identity.address, PORT))?;
    listener.set_nonblocking(true)?;
    Ok((listener, Arc::new(tls)))
}

fn unauthorized() -> Reply {
    ("401 Unauthorized", json!({"error":"Unauthorized phone"}))
}

fn valid_secret(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn allowed(method: &str, path: &str) -> bool {
    matches!(
        (method, path),
        (
            "GET",
            "/v1/status" | "/v1/events" | "/v1/presets" | "/v1/program?revision=1"
        ) | (
            "PUT",
            "/v1/setpoint" | "/v1/preset" | "/v1/hold" | "/v1/schedule" | "/v1/program"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_verifier_requires_approval_and_fails_after_revocation() {
        let root = std::env::temp_dir().join(format!("toonshift-remote-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let remote = Remote {
            root: root.clone(),
            state: Mutex::new(State {
                enabled: true,
                ..State::default()
            }),
        };
        remote.configure("192.168.8.9".parse().unwrap()).unwrap();
        remote.state.lock().unwrap().listening = true;
        remote.pair().unwrap();
        let generation = remote.state.lock().unwrap().generation;
        let qr: Value = serde_json::from_str(remote.view().qr.as_ref().unwrap()).unwrap();
        let secret = "a".repeat(64);
        let token = format!("{}.{secret}", qr["id"].as_str().unwrap());
        let claim = json!({"secret":qr["secret"],"name":"Fixture phone","verifier":format!("{:x}",Sha256::digest(secret.as_bytes()))});
        assert_eq!(
            remote.claim(generation, claim.to_string().as_bytes()).0,
            "202 Accepted"
        );
        assert_eq!(
            remote.claim(generation, claim.to_string().as_bytes()).0,
            "410 Gone"
        );
        assert!(remote.authorize(generation, &token).is_none());
        remote.approve().unwrap();
        assert!(remote.authorize(generation, &token).is_some());
        let stored: Identity = private_read(&root.join("remote-identity.json")).unwrap();
        assert_eq!(stored.phones.len(), 1);
        remote.revoke(qr["id"].as_str().unwrap()).unwrap();
        assert!(remote.authorize(generation, &token).is_none());
        remote.pair().unwrap();
        remote
            .state
            .lock()
            .unwrap()
            .pairing
            .as_mut()
            .unwrap()
            .expires = Instant::now();
        assert_eq!(
            remote.claim(generation, claim.to_string().as_bytes()).0,
            "410 Gone"
        );
        assert!(remote.approve().is_err());
        assert!(allowed("PUT", "/v1/hold"));
        assert!(!allowed("GET", "/v1/events?telemetry=1"));
        assert!(!allowed("GET", "/v1/diagnostics"));
        assert!(!allowed("PUT", "/v1/presets"));
        std::fs::remove_file(root.join("remote-identity.json")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
