//! Updates from GitHub releases.
//!
//! The only network access in Tessera. It asks GitHub for the latest release
//! at most once a day (switchable in the settings) and otherwise only when the
//! user presses the button.
//!
//! Nothing is installed unless two checks pass: `SHA256SUMS` must carry a valid
//! minisign signature from the key compiled into this program, and the
//! downloaded file must match its line in that list. A checksum on its own only
//! catches broken downloads; the signature is what stops a replaced release.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

const REPO: &str = "Benett53/Tessera";
/// minisign public key of the release pipeline (`MINISIGN_SECRET_KEY` secret).
const PUBLIC_KEY: &str = "RWSd/CoO3HCoOPHBb1md1slgD9my6lqvFJK+bXEjl0mIEYEXZjdcNecI";
const SUMS: &str = "SHA256SUMS";
const SIGNATURE: &str = "SHA256SUMS.minisig";
/// How long a check counts as recent.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub assets: Vec<Asset>,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

#[derive(Debug, Clone, Default)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    /// Bytes received and expected size, if known.
    Downloading(u64, Option<u64>),
    /// The new version is being installed; the program is about to quit.
    Installing,
    Failed(String),
}

/// Shared between the UI and the worker threads.
#[derive(Clone)]
pub struct Updater {
    status: Arc<Mutex<Status>>,
    /// Unix time of the last successful check, `0` if never.
    last_check: Arc<Mutex<u64>>,
    automatic: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    /// Set once an installer or replacement has been started.
    restart: Arc<AtomicBool>,
}

impl Updater {
    /// Starts the background loop that checks once a day while `automatic` is on.
    pub fn new<F>(automatic: bool, last_check: u64, wake: F) -> Self
    where
        F: Fn() + Send + Clone + 'static,
    {
        cleanup_old_executable();

        let updater = Self {
            status: Arc::default(),
            last_check: Arc::new(Mutex::new(last_check)),
            automatic: Arc::new(AtomicBool::new(automatic)),
            busy: Arc::default(),
            restart: Arc::default(),
        };

        let background = updater.clone();
        let _ = std::thread::Builder::new()
            .name("tessera-update-timer".into())
            .spawn(move || {
                // A short delay keeps the check away from startup work.
                std::thread::sleep(Duration::from_secs(20));
                loop {
                    if background.automatic.load(Ordering::Relaxed) && background.check_due() {
                        background.check(wake.clone());
                    }
                    std::thread::sleep(Duration::from_secs(60 * 60));
                }
            });

        updater
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn last_check(&self) -> u64 {
        self.last_check.lock().map(|t| *t).unwrap_or(0)
    }

    pub fn set_automatic(&self, on: bool) {
        self.automatic.store(on, Ordering::Relaxed);
    }

    /// The newer release, if one was found.
    pub fn available(&self) -> Option<Release> {
        match self.status() {
            Status::Available(release) => Some(release),
            _ => None,
        }
    }

    /// An installer was started and the program should quit.
    pub fn wants_exit(&self) -> bool {
        self.restart.load(Ordering::Relaxed)
    }

    fn check_due(&self) -> bool {
        now().saturating_sub(self.last_check()) >= CHECK_INTERVAL.as_secs()
    }

    fn set(&self, status: Status) {
        if let Ok(mut current) = self.status.lock() {
            *current = status;
        }
    }

    /// Ask GitHub for the latest release on a worker thread.
    pub fn check<F: Fn() + Send + 'static>(&self, wake: F) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = self.clone();
        self.set(Status::Checking);
        wake();
        std::thread::spawn(move || {
            let status = match latest_release() {
                Ok(release) if is_newer(&release.version, CURRENT_VERSION) => {
                    Status::Available(release)
                }
                Ok(_) => Status::UpToDate,
                Err(error) => Status::Failed(format!("Update check failed: {error}")),
            };
            if !matches!(status, Status::Failed(_))
                && let Ok(mut last) = this.last_check.lock()
            {
                *last = now();
            }
            this.set(status);
            this.busy.store(false, Ordering::Release);
            wake();
        });
    }

    /// Download, verify and start installing `release` on a worker thread.
    pub fn install<F: Fn() + Send + Clone + 'static>(&self, release: Release, wake: F) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = self.clone();
        self.set(Status::Downloading(0, None));
        wake();
        std::thread::spawn(move || {
            let progress = {
                let this = this.clone();
                let wake = wake.clone();
                move |received, total| {
                    this.set(Status::Downloading(received, total));
                    wake();
                }
            };
            match download_and_install(&release, progress) {
                Ok(()) => {
                    this.set(Status::Installing);
                    this.restart.store(true, Ordering::Release);
                }
                Err(error) => this.set(Status::Failed(format!("Update failed: {error}"))),
            }
            this.busy.store(false, Ordering::Release);
            wake();
        });
    }
}

type Result<T> = std::result::Result<T, String>;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .user_agent(concat!("Tessera/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn latest_release() -> Result<Release> {
    #[derive(serde::Deserialize)]
    struct ApiAsset {
        name: String,
        browser_download_url: String,
    }
    #[derive(serde::Deserialize)]
    struct ApiRelease {
        tag_name: String,
        assets: Vec<ApiAsset>,
    }

    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let text = agent()
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let api: ApiRelease = serde_json::from_str(&text).map_err(|e| e.to_string())?;

    Ok(Release {
        version: api.tag_name.trim_start_matches('v').to_string(),
        assets: api
            .assets
            .into_iter()
            .map(|a| Asset {
                name: a.name,
                url: a.browser_download_url,
            })
            .collect(),
    })
}

/// `1.10.0` is newer than `1.9.3`. Anything that does not parse is not newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parse(version: &str) -> Option<(u64, u64, u64)> {
        let core = version.split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        Some((
            parts.next()??,
            parts.next()??,
            parts.next().unwrap_or(Some(0))?,
        ))
    }
    match (parse(candidate), parse(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn fetch_text(url: &str) -> Result<String> {
    agent()
        .get(url)
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
}

/// The checksum list, but only if its signature is valid.
fn verified_sums(release: &Release) -> Result<String> {
    let sums_asset = release
        .asset(SUMS)
        .ok_or("the release has no checksum list")?;
    let sig_asset = release
        .asset(SIGNATURE)
        .ok_or("the release is not signed")?;
    let sums = fetch_text(&sums_asset.url)?;
    let signature = fetch_text(&sig_asset.url)?;
    verify_signature(sums.as_bytes(), &signature)?;
    Ok(sums)
}

fn verify_signature(data: &[u8], signature: &str) -> Result<()> {
    let key = minisign_verify::PublicKey::from_base64(PUBLIC_KEY).map_err(|e| e.to_string())?;
    let signature = minisign_verify::Signature::decode(signature).map_err(|e| e.to_string())?;
    key.verify(data, &signature, false)
        .map_err(|_| "the signature of the checksum list is not valid".to_string())
}

/// The expected SHA-256 of `name` in a `sha256sum` style list.
fn expected_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name).then(|| hash.to_ascii_lowercase())
    })
}

/// Stream `url` into `dest`, returning the SHA-256 of what was written.
fn download(url: &str, dest: &Path, progress: &impl Fn(u64, Option<u64>)) -> Result<String> {
    let response = agent().get(url).call().map_err(|e| e.to_string())?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    let mut reported = 0u64;
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        if received - reported > 256 * 1024 {
            reported = received;
            progress(received, total);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Download the right file for this copy, check it, and hand over to it.
fn download_and_install(release: &Release, progress: impl Fn(u64, Option<u64>)) -> Result<()> {
    let asset = release
        .asset(LINUX_ASSET)
        .ok_or_else(|| format!("{LINUX_ASSET} is missing from the release"))?;
    let sums = verified_sums(release)?;
    let expected = expected_hash(&sums, LINUX_ASSET)
        .ok_or_else(|| format!("{LINUX_ASSET} is not in the checksum list"))?;

    let dir = std::env::temp_dir().join(format!("tessera-update-{}", release.version));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(LINUX_ASSET);
    let actual = download(&asset.url, &file, &progress)?;
    if actual != expected {
        let _ = std::fs::remove_file(&file);
        return Err("the downloaded file does not match its checksum".into());
    }

    replace_executable(&file)
}

/// Release asset with the bare executable, used for updates. The tarball
/// next to it is for first installs.
pub const LINUX_ASSET: &str = "tessera-linux-x86_64";

/// Swap the executable for `new`.
///
/// Linux allows replacing a running executable by renaming over it: running
/// processes keep the old file open. The new file is first copied next to the
/// old one, so the rename stays on one file system. Without write access to
/// the folder (a system-wide install) `pkexec` asks for the password. The
/// caller restarts the service and the window afterwards.
fn replace_executable(new: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let staged = exe.with_file_name(".tessera.new");
    let local = std::fs::copy(new, &staged)
        .and_then(|_| std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)))
        .and_then(|_| std::fs::rename(&staged, &exe));

    if local.is_err() {
        let _ = std::fs::remove_file(&staged);
        let status = std::process::Command::new("pkexec")
            .arg("install")
            .arg("-m")
            .arg("755")
            .arg(new)
            .arg(&exe)
            .status()
            .map_err(|e| format!("pkexec: {e}"))?;
        if !status.success() {
            return Err("installation was not authorised".into());
        }
    }
    Ok(())
}

/// Removes what an earlier update left in the temp folder.
fn cleanup_old_executable() {
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("tessera-update-")
            {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0.0", "0.99.0"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        assert!(!is_newer("garbage", "0.3.0"));
        assert!(is_newer("0.3.1-rc1", "0.3.0"));
    }

    #[test]
    fn finds_hashes_in_both_sha256sum_formats() {
        let sums = "abc123  tessera-linux-x86_64\nDEF456 *tessera-linux-x86_64.tar.gz\n";
        assert_eq!(
            expected_hash(sums, "tessera-linux-x86_64").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            expected_hash(sums, "tessera-linux-x86_64.tar.gz").as_deref(),
            Some("def456")
        );
        assert_eq!(expected_hash(sums, "other"), None);
    }

    /// Signed with the release key, so this also proves `PUBLIC_KEY` matches it.
    const SIGNED: &[u8] = b"abc123  tessera-linux-x86_64\n";
    const SIGNATURE_OF_SIGNED: &str = r"untrusted comment: signature from rsign secret key
RUSd/CoO3HCoODMurssPJdy5BFxMuomVJugKj1JwvDlP421KORQZuveHqIvwV92j80X3/ZGdds/vd6GfNA1/PFsW+4eRbD2wNQw=
trusted comment: test
HbT3d+6iGLsb2Gjd0RDctdFWeJsuqiPOHNVTw8rIgKg3/FtQAvWui0j/mVv4Sa9QGb6KK4yXtu0UN1f74BKxCQ==
";

    #[test]
    fn accepts_a_release_signature() {
        assert!(verify_signature(SIGNED, SIGNATURE_OF_SIGNED).is_ok());
    }

    #[test]
    fn rejects_a_changed_list() {
        assert!(verify_signature(b"abc124  tessera-linux-x86_64\n", SIGNATURE_OF_SIGNED).is_err());
    }
}
