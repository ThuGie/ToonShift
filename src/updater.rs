use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const KEY: &str = "867a1cefbcbf02bfc8a997e0a381edde639d83ed9a1486002a55cafb4178538e";
const REPO: &str = "https://github.com/ThuGie/ToonShift/releases/download";
const LATEST: &str = "https://api.github.com/repos/ThuGie/ToonShift/releases/latest";
const BINARY: &str = "toonshift-armv7";
const MAX_BINARY: usize = 12 * 1024 * 1024;
const SUPERVISOR_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Version([u32; 3]);

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let items: Vec<_> = text.split('.').collect();
        if items.len() != 3 {
            return None;
        }
        let mut result = [0; 3];
        for (part, item) in result.iter_mut().zip(items) {
            *part = item.parse().ok()?;
            if part.to_string() != item {
                return None;
            }
        }
        Some(Self(result))
    }
}

#[derive(Deserialize)]
struct Manifest {
    schema: u32,
    version: String,
    commit: String,
    target: String,
    hardware: String,
    firmware: Vec<String>,
    minimum_updater: u32,
    binary: String,
    size: usize,
    sha256: String,
}

fn hex<const N: usize>(input: &str) -> Option<[u8; N]> {
    if input.len() != N * 2 || !input.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&input[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(bytes)
}

fn manifest(bytes: &[u8], sig: &[u8], version: &str) -> Result<Manifest, String> {
    verify_manifest(bytes, sig, version, &hex(KEY).expect("release key"))
}

fn verify_manifest(
    bytes: &[u8],
    sig: &[u8],
    version: &str,
    public_key: &[u8; 32],
) -> Result<Manifest, String> {
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| "Invalid release key")?;
    let sig = std::str::from_utf8(sig)
        .ok()
        .and_then(|sig| hex(sig.trim()))
        .ok_or("Invalid update signature")?;
    key.verify_strict(bytes, &Signature::from_bytes(&sig))
        .map_err(|_| "Invalid update signature")?;
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| "Invalid update manifest")?;
    if manifest.schema != 1
        || manifest.version != version
        || Version::parse(version).is_none()
        || hex::<20>(&manifest.commit).is_none()
        || manifest.hardware != "toon2"
        || manifest.target != "armv7-unknown-linux-musleabihf"
        || !manifest.firmware.iter().any(|version| version == "6.4.5")
        || !(1..=SUPERVISOR_VERSION).contains(&manifest.minimum_updater)
        || manifest.binary != BINARY
        || !(1..=MAX_BINARY).contains(&manifest.size)
        || hex::<32>(&manifest.sha256).is_none()
    {
        return Err("Unsupported update manifest".into());
    }
    Ok(manifest)
}

fn verify_binary(manifest: &Manifest, data: &[u8]) -> Result<(), String> {
    if data.len() != manifest.size
        || hex::<32>(&manifest.sha256).as_ref() != Some(&Sha256::digest(data).into())
    {
        return Err("Update checksum does not match".into());
    }
    Ok(())
}

fn atomic(path: &Path, bytes: &[u8], executable: bool) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let temp = path.with_extension(format!("{}.new", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .mode(if executable { 0o755 } else { 0o600 })
        .write(true)
        .truncate(true)
        .create(true)
        .open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(temp, path)?;
    sync_directory(path)
}

fn sync_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path.parent().expect("update directory"))?.sync_all()
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct State {
    highwater: Version,
    rejected: Option<Version>,
    rollback_reason: String,
}

fn read_state(root: &Path) -> io::Result<State> {
    match std::fs::read(root.join("update-state.json")) {
        Ok(data) => serde_json::from_slice(&data).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(State::default()),
        Err(error) => Err(error),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    atomic(
        path,
        &serde_json::to_vec(value).map_err(io::Error::other)?,
        false,
    )
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Staged {
    pub version: String,
    bytes: Vec<u8>,
    sig: Vec<u8>,
}

impl Staged {
    fn verified(&self, root: &Path) -> Result<Vec<u8>, String> {
        let manifest = manifest(&self.bytes, &self.sig, &self.version)?;
        let binary = read_limited(&root.join("update-candidate"), MAX_BINARY)?;
        verify_binary(&manifest, &binary)?;
        Ok(binary)
    }
}

fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut data))
        .map_err(|error| error.to_string())?;
    if data.len() > limit {
        return Err("Update file exceeds size limit".into());
    }
    Ok(data)
}

pub fn staged(root: &Path) -> Option<Staged> {
    let staged: Staged =
        serde_json::from_slice(&read_limited(&root.join("update-staged.json"), 65_536).ok()?)
            .ok()?;
    let state = read_state(root).ok()?;
    eligible(&staged.version, &state).then_some(staged)
}

fn eligible(version: &str, state: &State) -> bool {
    Version::parse(version).is_some_and(|version| {
        version
            > state
                .highwater
                .max(Version::parse(env!("CARGO_PKG_VERSION")).expect("build version"))
            && Some(version) != state.rejected
    })
}

pub fn rollback_reason(root: &Path) -> String {
    read_state(root).map_or_else(
        |_| "Update state unavailable".into(),
        |state| state.rollback_reason,
    )
}

fn host_allowed(url: &str) -> bool {
    let Some((host, path)) = url
        .strip_prefix("https://")
        .and_then(|url| url.split_once('/'))
    else {
        return false;
    };
    !path.is_empty()
        && !url
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b' ' | b'\\'))
        && matches!(
            host,
            "api.github.com"
                | "github.com"
                | "release-assets.githubusercontent.com"
                | "objects.githubusercontent.com"
                | "github-releases.githubusercontent.com"
        )
}

struct Response {
    status: u16,
    body: Vec<u8>,
    etag: Option<String>,
}

fn request(ca: &Path, url: &str, limit: usize, etag: Option<&str>) -> Result<Response, String> {
    let mut url = url.to_owned();
    for _ in 0..=4 {
        if !host_allowed(&url) {
            return Err("Update redirect is outside GitHub".into());
        }
        let mut command = Command::new("/usr/bin/curl");
        command
            .args([
                "-q",
                "-sS",
                "-i",
                "--http1.1",
                "--globoff",
                "--proto",
                "=https",
                "--noproxy",
                "*",
                "--connect-timeout",
                "8",
                "--max-time",
                "75",
                "--cacert",
            ])
            .arg(ca)
            .args(["--user-agent", "ToonShift-Updater/1"]);
        if let Some(etag) = etag
            .filter(|tag| url == LATEST && tag.len() < 256 && !tag.chars().any(char::is_control))
        {
            command
                .arg("--header")
                .arg(format!("If-None-Match: {etag}"));
        }
        let mut process = command
            .arg("--")
            .arg(&url)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "Could not start update download")?;
        let mut data = Vec::new();
        let read = process
            .stdout
            .take()
            .expect("piped stdout")
            .take(limit as u64 + 16_385)
            .read_to_end(&mut data);
        if read.is_err() || data.len() > limit + 16_384 {
            let _ = process.kill();
        }
        if !process.wait().is_ok_and(|status| status.success())
            || read.is_err()
            || data.len() > limit + 16_384
        {
            return Err("Update download failed or exceeded its size limit".into());
        }
        let header_end = data
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .filter(|offset| *offset <= 16_384)
            .ok_or("Invalid GitHub response")?;
        let headers =
            std::str::from_utf8(&data[..header_end]).map_err(|_| "Invalid GitHub headers")?;
        let status = headers
            .lines()
            .next()
            .and_then(|line| line.split_ascii_whitespace().nth(1))
            .and_then(|text| text.parse::<u16>().ok())
            .ok_or("Invalid GitHub status")?;
        let header = |name: &str| {
            headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.trim().to_owned())
        };
        match status {
            301 | 302 | 303 | 307 | 308 => {
                url = header("location").ok_or("Missing GitHub redirect")?
            }
            200 | 304 => {
                let etag = header("etag");
                let body = data.split_off(header_end + 4);
                if body.len() > limit {
                    return Err("Update download exceeded its size limit".into());
                }
                return Ok(Response { status, body, etag });
            }
            403 | 429 => return Err("GitHub rate limit or access restriction".into()),
            _ => return Err(format!("GitHub returned HTTP {status}")),
        }
    }
    Err("Too many update redirects".into())
}

pub fn check(root: &Path) -> Result<String, String> {
    if crate::thermostat::firmware_version()?.trim() != "6.4.5" {
        return Err("This firmware is outside the stable update track".into());
    }
    let ca = root.join("https-roots.pem");
    let etag = std::fs::read_to_string(root.join("update-etag")).ok();
    let response = request(&ca, LATEST, 131_072, etag.as_deref())?;
    if response.status == 304 {
        return Ok("No new stable release".into());
    }
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        draft: bool,
        prerelease: bool,
    }
    let release: Release =
        serde_json::from_slice(&response.body).map_err(|_| "Invalid GitHub release")?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .filter(|tag| Version::parse(tag).is_some())
        .ok_or("Invalid stable release tag")?;
    let state = read_state(root).map_err(|e| e.to_string())?;
    if release.draft || release.prerelease {
        return Err("GitHub did not return a stable release".into());
    }
    let mut message = "No new stable release";
    if eligible(version, &state) && !staged(root).is_some_and(|stage| stage.version == version) {
        let base = format!("{REPO}/{}", release.tag_name);
        let bytes = request(&ca, &format!("{base}/manifest.json"), 8192, None)?.body;
        let sig = request(&ca, &format!("{base}/manifest.sig"), 256, None)?.body;
        let description = manifest(&bytes, &sig, version)?;
        check_space(root, (description.size as u64) * 4 + 8 * 1024 * 1024)?;
        let binary = request(&ca, &format!("{base}/{BINARY}"), description.size, None)?.body;
        verify_binary(&description, &binary)?;
        atomic(&root.join("update-candidate"), &binary, true).map_err(|e| e.to_string())?;
        write_json(
            &root.join("update-staged.json"),
            &Staged {
                version: version.to_owned(),
                bytes,
                sig,
            },
        )
        .map_err(|e| e.to_string())?;
        message = "Update ready";
    }
    if let Some(etag) = response.etag {
        atomic(&root.join("update-etag"), etag.as_bytes(), false).map_err(|e| e.to_string())?;
    }
    Ok(message.into())
}

fn check_space(root: &Path, required: u64) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(root.as_os_str().as_bytes())
        .map_err(|_| "Invalid update directory")?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err("Could not check free space for the update".into());
    }
    let stats = unsafe { stats.assume_init() };
    if (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64) < required {
        return Err("Not enough free space for the update".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum Request {
    Install,
    Restore,
}

pub fn request_restart(root: &Path, request: Request) -> io::Result<()> {
    if std::env::var_os("TOONSHIFT_SUPERVISED").is_none() {
        return Err(io::Error::other("The update supervisor is not running"));
    }
    write_json(&root.join("update-request.json"), &request)
}

#[derive(Serialize, Deserialize)]
struct Pending {
    version: Option<Version>,
    attempted_restore: bool,
}

fn confirmed_version(root: &Path) -> Option<Version> {
    let mut child = Command::new(root.join("toon-rust"))
        .arg("--version")
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    if !wait_child(&mut child, Instant::now() + Duration::from_secs(3))
        .ok()
        .flatten()
        .is_some_and(|status| status.success())
    {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let mut output = String::new();
    child
        .stdout
        .take()?
        .take(256)
        .read_to_string(&mut output)
        .ok()?;
    Version::parse(output.split_ascii_whitespace().nth(1)?)
}

fn wait_child(
    child: &mut Child,
    deadline: Instant,
) -> io::Result<Option<std::process::ExitStatus>> {
    loop {
        let status = child.try_wait()?;
        if status.is_some() || Instant::now() >= deadline {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn launch(root: &Path) -> io::Result<Child> {
    supervised_child(Command::new(root.join("toon-rust")).env("TOONSHIFT_SUPERVISED", "1"))
}

fn supervised_child(command: &mut Command) -> io::Result<Child> {
    use std::os::unix::process::CommandExt;
    let parent = std::process::id() as libc::pid_t;
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 || libc::getppid() != parent
            {
                return Err(io::Error::other("Update supervisor unavailable"));
            }
            Ok(())
        });
    }
    command.spawn()
}

fn clear(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => sync_directory(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn rollback(root: &Path, reason: &str) -> io::Result<()> {
    let path = root.join("update-pending.json");
    let pending: Pending =
        serde_json::from_slice(&std::fs::read(&path)?).map_err(io::Error::other)?;
    let old = read_limited(&root.join("update-previous"), MAX_BINARY).map_err(io::Error::other)?;
    atomic(&root.join("toon-rust"), &old, true)?;
    let mut state = read_state(root)?;
    if !pending.attempted_restore {
        state.rejected = pending.version;
    }
    state.rollback_reason = reason.into();
    write_json(&root.join("update-state.json"), &state)?;
    clear(&path)
}

pub fn ready(root: &Path) -> io::Result<()> {
    if std::env::var_os("TOONSHIFT_TRIAL").is_some() {
        atomic(&root.join("update-ready"), b"ready\n", false)?;
    }
    Ok(())
}

pub fn supervise(root: &Path) -> io::Result<()> {
    let pending_path = root.join("update-pending.json");
    let request_path = root.join("update-request.json");
    let previous = root.join("update-previous");
    clear(&request_path)?;
    if pending_path.exists() {
        rollback(root, "Interrupted update restored")?;
    }
    let mut running = launch(root)?;
    loop {
        running.wait()?;
        let request: Request = match std::fs::read(&request_path) {
            Ok(data) => serde_json::from_slice(&data).map_err(io::Error::other)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        clear(&request_path)?;
        let current = confirmed_version(root);
        let install = matches!(request, Request::Install);
        let candidate = if install {
            staged(root)
                .ok_or("No verified update is staged")
                .and_then(|staged| {
                    let version = Version::parse(&staged.version);
                    staged
                        .verified(root)
                        .map(|data| (data, version))
                        .map_err(|_| "Staged update verification failed")
                })
        } else {
            read_limited(&previous, MAX_BINARY)
                .map(|data| (data, current))
                .map_err(|_| "Previous version unavailable")
        };
        let (candidate, version) = match candidate {
            Ok(candidate) => candidate,
            Err(reason) => {
                let mut state = read_state(root)?;
                state.rollback_reason = reason.into();
                write_json(&root.join("update-state.json"), &state)?;
                running = launch(root)?;
                continue;
            }
        };
        let probe_path = root.join("update-probe");
        atomic(&probe_path, &candidate, true)?;
        let mut probe = Command::new(&probe_path)
            .arg("--probe")
            .stdout(Stdio::null())
            .spawn()?;
        let healthy = match wait_child(&mut probe, Instant::now() + Duration::from_secs(20))? {
            Some(status) => status.success(),
            None => {
                let _ = probe.kill();
                let _ = probe.wait();
                false
            }
        };
        clear(&probe_path)?;
        if !healthy {
            let mut state = read_state(root)?;
            if install {
                state.rejected = version;
            }
            state.rollback_reason = "Update probe failed".into();
            write_json(&root.join("update-state.json"), &state)?;
            running = launch(root)?;
            continue;
        }
        let active = root.join("toon-rust");
        let old = read_limited(&active, MAX_BINARY).map_err(io::Error::other)?;
        atomic(&previous, &old, true)?;
        write_json(
            &pending_path,
            &Pending {
                version,
                attempted_restore: !install,
            },
        )?;
        atomic(&active, &candidate, true)?;
        let marker = root.join("update-ready");
        clear(&marker)?;
        let mut trial = match supervised_child(
            Command::new(&active)
                .env("TOONSHIFT_SUPERVISED", "1")
                .env("TOONSHIFT_TRIAL", "1"),
        ) {
            Ok(trial) => trial,
            Err(_) => {
                rollback(root, "Update could not start")?;
                running = launch(root)?;
                continue;
            }
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut healthy = false;
        loop {
            if trial.try_wait()?.is_some() {
                break;
            }
            if marker.exists() {
                healthy =
                    wait_child(&mut trial, Instant::now() + Duration::from_secs(60))?.is_none();
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        clear(&marker)?;
        if !healthy {
            let _ = trial.kill();
            let _ = trial.wait();
            clear(&request_path)?;
            rollback(root, "Update failed startup checks")?;
            running = launch(root)?;
            continue;
        }
        let mut state = read_state(root)?;
        if let Some(version) = version {
            state.highwater = state.highwater.max(version);
        }
        state.rollback_reason.clear();
        write_json(&root.join("update-state.json"), &state)?;
        clear(&pending_path)?;
        running = trial;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn signed_manifest_binds_version_target_and_binary() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let binary = b"fixture";
        let payload = serde_json::json!({
            "schema":1,"version":"0.2.1","commit":"a".repeat(40),
            "target":"armv7-unknown-linux-musleabihf","hardware":"toon2",
            "firmware":["6.4.5"],"minimum_updater":1,"binary":BINARY,
            "size":binary.len(),"sha256":format!("{:x}", Sha256::digest(binary))
        })
        .to_string();
        let signature = |data: &[u8]| {
            key.sign(data)
                .to_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let sig = signature(payload.as_bytes());
        let public = key.verifying_key().to_bytes();
        let decoded =
            verify_manifest(payload.as_bytes(), sig.as_bytes(), "0.2.1", &public).unwrap();
        assert!(verify_binary(&decoded, binary).is_ok());
        assert!(verify_binary(&decoded, b"corrupt").is_err());
        assert!(verify_manifest(payload.as_bytes(), sig.as_bytes(), "0.2.2", &public).is_err());
        let wrong = payload.replace("toon2", "toon1");
        assert!(verify_manifest(wrong.as_bytes(), sig.as_bytes(), "0.2.1", &public).is_err());
        assert!(verify_manifest(
            wrong.as_bytes(),
            signature(wrong.as_bytes()).as_bytes(),
            "0.2.1",
            &public
        )
        .is_err());
    }

    #[test]
    fn rejects_unsafe_redirects_and_older_or_failed_versions() {
        assert!(host_allowed(
            "https://release-assets.githubusercontent.com/x"
        ));
        for url in [
            "http://github.com/x",
            "https://github.com.example/x",
            "https://github.com@evil.test/x",
            "https://github.com:443/x",
        ] {
            assert!(!host_allowed(url), "{url}");
        }
        let state = State {
            highwater: Version([1, 3, 0]),
            rejected: Some(Version([1, 4, 0])),
            rollback_reason: String::new(),
        };
        for tag in ["1.2.9", "1.3.0", "1.4.0", "1.4.1-rc.1", "1.4.01"] {
            assert!(!eligible(tag, &state), "{tag}");
        }
        assert!(eligible("1.4.1", &state));
    }

    #[test]
    fn interrupted_activation_restores_the_previous_binary() {
        let root =
            std::env::temp_dir().join(format!("toonshift-updater-test-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        atomic(&root.join("toon-rust"), b"candidate", true).unwrap();
        atomic(&root.join("update-previous"), b"known-good", true).unwrap();
        write_json(
            &root.join("update-pending.json"),
            &Pending {
                version: Some(Version([1, 2, 3])),
                attempted_restore: false,
            },
        )
        .unwrap();
        rollback(&root, "interrupted").unwrap();
        assert_eq!(
            std::fs::read(root.join("toon-rust")).unwrap(),
            b"known-good"
        );
        assert!(!root.join("update-pending.json").exists());
        assert_eq!(
            read_state(&root).unwrap().rejected,
            Some(Version([1, 2, 3]))
        );
        for name in ["toon-rust", "update-previous", "update-state.json"] {
            std::fs::remove_file(root.join(name)).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }
}
