use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Cursor, Read, Seek, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use bzip2::read::BzDecoder;
use chrono::{Local, SecondsFormat, Utc};
use fs2::FileExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ureq::ResponseExt;
use zip::ZipArchive;

use crate::{application, config};

const GSI_HOST: &str = "127.0.0.1";
const GSI_PORT: u16 = 31982;
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(30);
const POST_MATCH_WINDOW: Duration = Duration::from_secs(10 * 60);
const IDLE_DISCOVERY_INTERVAL: Duration = Duration::from_secs(15 * 60);
const HELPER_URL: &str = "https://github.com/akiver/boiler-writter/releases/download/v1.7.0/boiler-writter-linux-1.7.0.zip";
const HELPER_SHA256: &str = "f3c85acebb55a8c8eefb1334db4fce2cda397dc50eb8ecdb5664cedbc900f7ff";
const VRF_VERSION: &str = "20.0";
const VRF_URL: &str = "https://github.com/ValveResourceFormat/ValveResourceFormat/releases/download/20.0/cli-linux-x64.zip";
const VRF_SHA256: &str = "3e8af47cd6ce52e8068904f2aa1dda23c56a6b96a8310b25090f0711cda76a8a";
const MAX_COMPRESSED_DEMO_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DEMO_BYTES: u64 = 2 * 1024 * 1024 * 1024;

fn runtime_root() -> PathBuf {
    config::state_home().join("omarcs")
}
fn demos_root() -> PathBuf {
    config::data_home().join("omarcs/demos")
}
fn helper_root() -> PathBuf {
    config::data_home().join("omarcs/boiler-writter")
}
fn state_path() -> PathBuf {
    runtime_root().join("autofetch.json")
}
fn utc_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutoState {
    pub known_urls: Vec<String>,
    pub queue: Vec<String>,
    pub status: String,
    pub error: String,
    pub current: String,
    pub updated_at: String,
    pub last_poll_at: String,
    pub last_imported_at: String,
    pub game_phase: String,
    pub pid: u32,
    pub gsi: String,
}

pub fn load_state() -> AutoState {
    fs::read_to_string(state_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_state(state: &AutoState) -> Result<()> {
    let root = runtime_root();
    fs::create_dir_all(&root)?;
    let temporary = root.join(format!("autofetch.{}.json", std::process::id()));
    let mut output = File::create(&temporary)?;
    serde_json::to_writer_pretty(&mut output, state)?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    fs::rename(temporary, state_path())?;
    Ok(())
}

pub fn extract_replay_urls(payload: &[u8]) -> Vec<String> {
    let expression =
        Regex::new(r"https?://replay\d+\.valve\.net/730/[A-Za-z0-9_-]+\.dem\.bz2").unwrap();
    let text = String::from_utf8_lossy(payload);
    let mut found = Vec::new();
    for item in expression
        .find_iter(&text)
        .map(|item| item.as_str().to_owned())
    {
        if !found.contains(&item) {
            found.push(item);
        }
    }
    found
}

pub fn trusted_replay_url(url: &str) -> bool {
    Regex::new(r"^https?://replay\d+\.valve\.net/730/[A-Za-z0-9_-]+\.dem\.bz2$")
        .unwrap()
        .is_match(url)
}

fn helper_binary() -> PathBuf {
    helper_root().join("boiler-writter")
}

fn query_recent_urls() -> Result<Vec<String>> {
    let binary = helper_binary();
    if !binary.exists() {
        bail!("Match helper is not installed; run `omarcs setup-auto`");
    }
    fs::create_dir_all(helper_root())?;
    let output = helper_root().join(format!("recent.{}.info", std::process::id()));
    File::create(&output)?;
    let mut child = Command::new(&binary)
        .arg(output.file_name().unwrap())
        .current_dir(helper_root())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("running {}", binary.display()))?;
    let deadline = Instant::now() + Duration::from_secs(25);
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            let _ = fs::remove_file(&output);
            bail!("match helper timed out");
        }
        thread::sleep(Duration::from_millis(100));
    }
    let result = child.wait_with_output()?;
    if !result.status.success() {
        let detail = String::from_utf8_lossy(&result.stderr)
            .lines()
            .last()
            .unwrap_or("match helper failed")
            .to_owned();
        let _ = fs::remove_file(&output);
        bail!(detail);
    }
    let payload = fs::read(&output)?;
    let _ = fs::remove_file(&output);
    let urls = extract_replay_urls(&payload);
    if urls.is_empty() {
        bail!("Steam returned no recent Valve match replays");
    }
    Ok(urls)
}

#[derive(Debug)]
struct GameStateData {
    phase: String,
    updated_at: Instant,
    gameover_at: Option<Instant>,
    process_checked_at: Instant,
    process_running: bool,
}

#[derive(Clone)]
pub struct GameState(Arc<Mutex<GameStateData>>);

impl GameState {
    pub fn new() -> Self {
        let now = Instant::now();
        Self(Arc::new(Mutex::new(GameStateData {
            phase: "unknown".to_owned(),
            updated_at: now,
            gameover_at: None,
            process_checked_at: now.checked_sub(Duration::from_secs(3)).unwrap_or(now),
            process_running: false,
        })))
    }

    pub fn update(&self, payload: &Value) {
        let Some(phase) = payload
            .get("map")
            .and_then(|map| map.get("phase"))
            .and_then(Value::as_str)
            .filter(|phase| !phase.is_empty())
        else {
            return;
        };
        let mut state = self.0.lock().unwrap();
        let phase = phase.to_lowercase();
        if phase == "gameover" && state.phase != "gameover" {
            state.gameover_at = Some(Instant::now());
        }
        state.phase = phase;
        state.updated_at = Instant::now();
    }

    fn snapshot(&self) -> (String, Instant, Option<Instant>) {
        let state = self.0.lock().unwrap();
        (state.phase.clone(), state.updated_at, state.gameover_at)
    }

    pub fn heavy_work_allowed(&self) -> bool {
        let mut state = self.0.lock().unwrap();
        let now = Instant::now();
        if now.duration_since(state.process_checked_at) >= Duration::from_secs(2) {
            state.process_running = cs2_running().unwrap_or(true);
            state.process_checked_at = now;
        }
        if matches!(state.phase.as_str(), "warmup" | "live" | "intermission") {
            return !state.process_running
                && now.duration_since(state.updated_at) > Duration::from_secs(30);
        }
        if state.phase == "unknown" && state.process_running {
            return false;
        }
        true
    }
}

fn cs2_running() -> Result<bool> {
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        if fs::read_to_string(entry.path().join("comm"))
            .ok()
            .is_some_and(|name| name.trim() == "cs2")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn start_gsi_server(
    game_state: GameState,
    stop: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>> {
    let listener = TcpListener::bind((GSI_HOST, GSI_PORT))
        .context("binding the CS2 Game State Integration listener")?;
    listener.set_nonblocking(true)?;
    Ok(thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let state = game_state.clone();
                    thread::spawn(move || {
                        let _ = handle_gsi(stream, &state);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100))
                }
                Err(_) => break,
            }
        }
    }))
}

fn handle_gsi(mut stream: TcpStream, game_state: &GameState) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let post = request_line.starts_with("POST ");
    let mut content_length = 0_usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse::<usize>().unwrap_or(0).min(1024 * 1024);
        }
    }
    let mut body = vec![0; content_length];
    let valid = post
        && reader.read_exact(&mut body).is_ok()
        && serde_json::from_slice::<Value>(&body)
            .map(|payload| game_state.update(&payload))
            .is_ok();
    let response = if valid {
        "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    } else {
        "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    };
    stream.write_all(response.as_bytes())?;
    Ok(())
}

#[derive(Debug)]
struct InterruptedForGame;
impl std::fmt::Display for InterruptedForGame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "work paused because a match started")
    }
}
impl std::error::Error for InterruptedForGame {}

fn download_demo(url: &str, game_state: &GameState) -> Result<PathBuf> {
    if !trusted_replay_url(url) {
        bail!("Expected a trusted Valve replay URL");
    }
    let root = demos_root();
    fs::create_dir_all(&root)?;
    let name = url
        .rsplit('/')
        .next()
        .ok_or_else(|| anyhow!("Replay URL has no filename"))?;
    let compressed = root.join(name);
    let partial = compressed.with_extension("bz2.part");
    let mut offset = partial
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if offset > MAX_COMPRESSED_DEMO_BYTES {
        fs::remove_file(&partial)?;
        bail!("Partial replay exceeds the compressed size limit");
    }
    let mut request = ureq::get(url).header("User-Agent", "omarCS/0.2");
    if offset > 0 {
        request = request.header("Range", &format!("bytes={offset}-"));
    }
    let mut response = request.call().context("downloading Valve replay")?;
    let final_url = response.get_uri().to_string();
    if !trusted_replay_url(&final_url) {
        bail!("Replay download redirected outside trusted Valve replay hosts");
    }
    let append = offset > 0 && response.status().as_u16() == 206;
    if !append {
        offset = 0;
    }
    let mut output = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&partial)?;
    let mut reader = response.body_mut().as_reader();
    let mut buffer = [0_u8; 1024 * 1024];
    let mut written = offset;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        if !game_state.heavy_work_allowed() {
            return Err(InterruptedForGame.into());
        }
        written += count as u64;
        if written > MAX_COMPRESSED_DEMO_BYTES {
            bail!("Replay exceeds the compressed size limit");
        }
        output.write_all(&buffer[..count])?;
    }
    drop(output);
    let mut magic = [0; 3];
    File::open(&partial)?.read_exact(&mut magic)?;
    if written < 4 || &magic != b"BZh" {
        let _ = fs::remove_file(&partial);
        bail!("Downloaded replay is not a bzip2 stream");
    }
    fs::rename(&partial, &compressed)?;
    Ok(compressed)
}

fn decompress_demo(compressed: &Path, game_state: &GameState) -> Result<PathBuf> {
    decompress_demo_with_limit(compressed, MAX_DEMO_BYTES, || {
        game_state.heavy_work_allowed()
    })
}

fn decompress_demo_with_limit(
    compressed: &Path,
    maximum: u64,
    allowed: impl Fn() -> bool,
) -> Result<PathBuf> {
    let demo = compressed.with_extension("");
    let partial = demo.with_extension("dem.part");
    let result = (|| -> Result<()> {
        let mut source = BzDecoder::new(File::open(compressed)?);
        let mut output = File::create(&partial)?;
        let mut buffer = [0_u8; 1024 * 1024];
        let mut written = 0_u64;
        loop {
            let count = source.read(&mut buffer).context("decompressing replay")?;
            if count == 0 {
                break;
            }
            if !allowed() {
                return Err(InterruptedForGame.into());
            }
            written += count as u64;
            if written > maximum {
                bail!("Replay exceeds the decompressed size limit");
            }
            output.write_all(&buffer[..count])?;
        }
        drop(output);
        let mut header = [0; 8];
        let read = File::open(&partial)?.read(&mut header)?;
        if read != header.len() || &header != b"PBDEMS2\0" {
            bail!("Decompressed replay does not have a CS2 demo header");
        }
        fs::rename(&partial, &demo)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result.map(|()| demo)
}

fn parse_demo(demo: &Path, game_state: &GameState) -> Result<()> {
    let executable = std::env::current_exe()?;
    let mut child = Command::new(executable)
        .args(["import", &demo.to_string_lossy(), "--quiet"])
        .spawn()?;
    wait_for_import(&mut child, game_state)
}

fn wait_for_import(child: &mut Child, game_state: &GameState) -> Result<()> {
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("parser exited {}", status.code().unwrap_or(1));
            }
            return Ok(());
        }
        if !game_state.heavy_work_allowed() {
            child.kill()?;
            child.wait()?;
            return Err(InterruptedForGame.into());
        }
        thread::sleep(Duration::from_millis(250));
    }
}

struct AutoFetcher {
    game_state: GameState,
    state: AutoState,
}
impl AutoFetcher {
    fn new(game_state: GameState) -> Self {
        Self {
            game_state,
            state: load_state(),
        }
    }
    fn record(&mut self, update: impl FnOnce(&mut AutoState)) -> Result<()> {
        update(&mut self.state);
        self.state.updated_at = utc_now();
        save_state(&self.state)
    }
    fn discover(&mut self) -> Result<()> {
        self.record(|state| {
            state.status = "polling".to_owned();
            state.error.clear();
        })?;
        let urls = query_recent_urls()?;
        let mut queue = self.state.queue.clone();
        for url in urls
            .iter()
            .rev()
            .filter(|url| !self.state.known_urls.contains(url))
        {
            if !queue.contains(url) {
                queue.push(url.clone());
            }
        }
        let mut known = urls;
        known.extend(self.state.known_urls.clone());
        let mut seen = std::collections::HashSet::new();
        known.retain(|url| seen.insert(url.clone()));
        known.truncate(40);
        self.record(|state| {
            state.status = if queue.is_empty() { "idle" } else { "queued" }.to_owned();
            state.known_urls = known;
            state.queue = queue;
            state.last_poll_at = utc_now();
        })
    }
    fn process_queue(&mut self) -> Result<()> {
        while !self.state.queue.is_empty() && self.game_state.heavy_work_allowed() {
            let url = self.state.queue[0].clone();
            let name = url.rsplit('/').next().unwrap_or("match.dem.bz2").to_owned();
            self.record(|state| {
                state.status = "downloading".to_owned();
                state.current = name.clone();
            })?;
            let mut compressed = demos_root().join(&name);
            let demo = compressed.with_extension("");
            let demo = if demo.exists() {
                demo
            } else {
                if !compressed.exists() {
                    compressed = download_demo(&url, &self.game_state)?;
                }
                self.record(|state| {
                    state.status = "decompressing".to_owned();
                    state.current = name.clone();
                })?;
                decompress_demo(&compressed, &self.game_state)?
            };
            self.record(|state| {
                state.status = "parsing".to_owned();
                state.current = demo
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            })?;
            parse_demo(&demo, &self.game_state)?;
            let _ = fs::remove_file(&demo);
            let _ = fs::remove_file(&compressed);
            self.state.queue.remove(0);
            self.record(|state| {
                state.status = "idle".to_owned();
                state.current.clear();
                state.last_imported_at = utc_now();
            })?;
        }
        Ok(())
    }
}

pub fn run_daemon() -> Result<u8> {
    fs::create_dir_all(runtime_root())?;
    let lock_file = File::create(runtime_root().join("autofetch.lock"))?;
    if lock_file.try_lock_exclusive().is_err() {
        eprintln!("omarCS auto-fetch is already running");
        return Ok(1);
    }
    let game_state = GameState::new();
    let stop = Arc::new(AtomicBool::new(false));
    let server = start_gsi_server(game_state.clone(), stop.clone())?;
    let mut fetcher = AutoFetcher::new(game_state.clone());
    fetcher.record(|state| {
        state.status = "idle".to_owned();
        state.pid = std::process::id();
        state.gsi = format!("http://{GSI_HOST}:{GSI_PORT}/gsi");
        state.game_phase = "unknown".to_owned();
        state.error.clear();
    })?;
    let mut next_poll = Instant::now() + Duration::from_secs(15);
    let mut handled_gameover = None;
    let mut burst_until = Instant::now();
    let mut recorded_phase = "unknown".to_owned();
    let mut work_retry_at = Instant::now();
    let mut work_failures = 0_u32;
    loop {
        let now = Instant::now();
        let (phase, _, gameover_at) = game_state.snapshot();
        if phase != recorded_phase {
            recorded_phase.clone_from(&phase);
            fetcher.record(|state| {
                state.game_phase = phase.clone();
                state.status = if matches!(phase.as_str(), "warmup" | "live" | "intermission") {
                    "in-game"
                } else if phase == "gameover" {
                    "waiting"
                } else {
                    "idle"
                }
                .to_owned();
                if state.status == "in-game" {
                    state.error.clear();
                }
            })?;
        }
        if let Some(gameover) = gameover_at.filter(|gameover| Some(*gameover) != handled_gameover) {
            handled_gameover = Some(gameover);
            next_poll = gameover + DISCOVERY_INTERVAL;
            burst_until = gameover + POST_MATCH_WINDOW;
        }
        if game_state.heavy_work_allowed() {
            let work = (|| -> Result<()> {
                if now >= work_retry_at {
                    fetcher.process_queue()?;
                    if fetcher.state.queue.is_empty() {
                        work_failures = 0;
                    }
                }
                if now >= next_poll {
                    fetcher.discover()?;
                    if now >= work_retry_at {
                        fetcher.process_queue()?;
                        if fetcher.state.queue.is_empty() {
                            work_failures = 0;
                        }
                    }
                    next_poll = now
                        + if now < burst_until {
                            DISCOVERY_INTERVAL
                        } else {
                            IDLE_DISCOVERY_INTERVAL
                        };
                }
                Ok(())
            })();
            if let Err(error) = work {
                work_failures += 1;
                let interrupted = error.downcast_ref::<InterruptedForGame>().is_some();
                let detail = error.to_string();
                fetcher.record(|state| {
                    state.status = if interrupted { "paused" } else { "waiting" }.to_owned();
                    state.error = detail;
                    state.last_poll_at = utc_now();
                })?;
                let factor = 2_u32.saturating_pow(work_failures.saturating_sub(1).min(10));
                work_retry_at = now
                    + DISCOVERY_INTERVAL
                        .saturating_mul(factor)
                        .min(IDLE_DISCOVERY_INTERVAL);
                next_poll = now
                    + if now < burst_until {
                        DISCOVERY_INTERVAL
                    } else {
                        IDLE_DISCOVERY_INTERVAL
                    };
            }
        }
        thread::sleep(Duration::from_millis(500));
        if stop.load(Ordering::Relaxed) {
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = server.join();
    Ok(0)
}

fn download_bytes(url: &str, maximum: u64) -> Result<Vec<u8>> {
    let mut response = ureq::get(url).header("User-Agent", "omarCS/0.2").call()?;
    let mut reader = response.body_mut().as_reader();
    let mut output = Vec::new();
    reader.by_ref().take(maximum + 1).read_to_end(&mut output)?;
    if output.len() as u64 > maximum {
        bail!("Downloaded archive exceeds its size limit");
    }
    Ok(output)
}

fn verify_sha256(bytes: &[u8], expected: &str, label: &str) -> Result<()> {
    if format!("{:x}", Sha256::digest(bytes)) != expected {
        bail!("Downloaded {label} failed its SHA-256 check");
    }
    Ok(())
}

fn extract_named<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    target_root: &Path,
    names: &[&str],
) -> Result<()> {
    for wanted in names {
        let index = (0..archive.len())
            .find(|index| {
                archive
                    .by_index(*index)
                    .ok()
                    .and_then(|file| {
                        Path::new(file.name())
                            .file_name()
                            .map(|name| name == *wanted)
                    })
                    .unwrap_or(false)
            })
            .ok_or_else(|| anyhow!("Downloaded archive is missing {wanted}"))?;
        let mut source = archive.by_index(index)?;
        let target = target_root.join(wanted);
        let mut output = File::create(&target)?;
        std::io::copy(&mut source, &mut output)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(
                    if *wanted == "boiler-writter" || *wanted == "Source2Viewer-CLI" {
                        0o755
                    } else {
                        0o644
                    },
                ),
            )?;
        }
    }
    Ok(())
}

fn install_helper() -> Result<()> {
    let root = helper_root();
    fs::create_dir_all(&root)?;
    let required = ["boiler-writter", "libsteam_api.so", "steam_appid.txt"];
    if required.iter().all(|name| root.join(name).is_file()) {
        return Ok(());
    }
    let archive = download_bytes(HELPER_URL, 100 * 1024 * 1024)?;
    verify_sha256(&archive, HELPER_SHA256, "match helper")?;
    extract_named(
        &mut ZipArchive::new(Cursor::new(archive))?,
        &root,
        &required,
    )
}

fn install_vrf() -> Result<()> {
    let root = config::data_home().join("omarcs/vrf");
    fs::create_dir_all(&root)?;
    let required = ["Source2Viewer-CLI", "libSkiaSharp.so", "libspirv-cross.so"];
    if fs::read_to_string(root.join(".version"))
        .ok()
        .is_some_and(|value| value.trim() == VRF_VERSION)
        && required.iter().all(|name| root.join(name).exists())
    {
        return Ok(());
    }
    let archive = download_bytes(VRF_URL, 256 * 1024 * 1024)?;
    verify_sha256(&archive, VRF_SHA256, "map geometry helper")?;
    extract_named(
        &mut ZipArchive::new(Cursor::new(archive))?,
        &root,
        &required,
    )?;
    fs::write(root.join(".version"), format!("{VRF_VERSION}\n"))?;
    Ok(())
}

fn steam_gsi_path() -> Result<PathBuf> {
    for candidate in [
        config::data_home()
            .join("Steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg"),
        config::home_dir()
            .join(".steam/steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg"),
    ] {
        if candidate.exists() {
            return Ok(candidate.join("gamestate_integration_omarcs.cfg"));
        }
    }
    bail!("Could not find the local CS2 cfg directory")
}

fn install_text_file(path: &Path, content: &str) -> Result<()> {
    if path.exists() && fs::read_to_string(path).unwrap_or_default() != content {
        let backup = runtime_root().join("backups");
        fs::create_dir_all(&backup)?;
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        fs::copy(
            path,
            backup.join(format!(
                "{}.{}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                stamp
            )),
        )?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

pub fn setup_auto(seed: bool) -> Result<()> {
    install_helper()?;
    install_vrf()?;
    let gsi = steam_gsi_path()?;
    install_text_file(
        &gsi,
        &format!(
            "\"omarCS\"\n{{\n  \"uri\" \"http://{GSI_HOST}:{GSI_PORT}/gsi\"\n  \"timeout\" \"1.0\"\n  \"buffer\" \"0.2\"\n  \"throttle\" \"1.0\"\n  \"heartbeat\" \"10.0\"\n  \"data\"\n  {{\n    \"provider\" \"1\"\n    \"map\" \"1\"\n  }}\n}}\n"
        ),
    )?;
    let mut state = load_state();
    if seed && state.known_urls.is_empty() {
        state.known_urls = query_recent_urls()?;
        state.queue.clear();
        state.last_poll_at = utc_now();
        state.status = "idle".to_owned();
        save_state(&state)?;
    }
    let executable = std::env::current_exe()?.canonicalize()?;
    let unit = config::home_dir().join(".config/systemd/user/omarcs-autofetch.service");
    install_text_file(
        &unit,
        &format!(
            "[Unit]\nDescription=omarCS automatic Valve demo fetcher\nAfter=network-online.target\n\n[Service]\nExecStart={} auto-run\nRestart=on-failure\nRestartSec=10\nNice=10\nIOSchedulingClass=idle\nCPUWeight=10\nIOWeight=10\n\n[Install]\nWantedBy=default.target\n",
            executable.display()
        ),
    )?;
    Ok(())
}

pub fn enable_daemon() -> Result<()> {
    for arguments in [
        ["--user", "daemon-reload"].as_slice(),
        ["--user", "enable", "omarcs-autofetch.service"].as_slice(),
        ["--user", "restart", "omarcs-autofetch.service"].as_slice(),
    ] {
        let status = Command::new("systemctl").args(arguments).status()?;
        if !status.success() {
            bail!("systemctl {} failed", arguments.join(" "));
        }
    }
    Ok(())
}

pub fn remove_legacy_runtime() -> Result<()> {
    let old_binary = config::data_home().join("omarcs/omarcs-native");
    if old_binary.exists() {
        fs::remove_file(old_binary)?;
    }
    let old_environment = config::cache_home().join("omarcs/venv");
    if old_environment.exists() {
        fs::remove_dir_all(old_environment)?;
    }
    Ok(())
}

pub fn bootstrap() -> Result<u8> {
    setup_auto(true)?;
    enable_daemon()?;
    remove_legacy_runtime()?;
    let settings = config::load_settings(None)?;
    application::import_paths(&settings.import_paths, None, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bzip2::write::BzEncoder;
    use bzip2::Compression;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("omarcs-{name}-{}-{stamp}", std::process::id()))
    }

    fn compressed(path: &Path, contents: &[u8]) {
        let mut encoder = BzEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(contents).unwrap();
        fs::write(path, encoder.finish().unwrap()).unwrap();
    }

    #[test]
    fn extracts_unique_urls_in_order() {
        let first = b"http://replay423.valve.net/730/one.dem.bz2";
        let second = b"https://replay171.valve.net/730/two.dem.bz2";
        let mut payload = b"http://replay999.valve.net.evil.test/730/unsafe.dem.bz2\0".to_vec();
        payload.extend(first);
        payload.push(0);
        payload.extend(second);
        payload.push(0);
        payload.extend(first);
        assert_eq!(
            extract_replay_urls(&payload),
            vec![
                String::from_utf8(first.to_vec()).unwrap(),
                String::from_utf8(second.to_vec()).unwrap()
            ]
        );
    }
    #[test]
    fn only_exact_valve_replay_urls_are_trusted() {
        assert!(trusted_replay_url(
            "http://replay423.valve.net/730/match.dem.bz2"
        ));
        assert!(trusted_replay_url(
            "https://replay423.valve.net/730/match.dem.bz2"
        ));
        assert!(!trusted_replay_url(
            "https://replay423.valve.net.evil.test/730/match.dem.bz2"
        ));
        assert!(!trusted_replay_url(
            "https://replay423.valve.net/730/../match.dem.bz2"
        ));
    }

    #[test]
    fn decompression_enforces_expansion_limit_and_cleans_partial_file() {
        let root = temporary_root("decompress-limit");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("match.dem.bz2");
        compressed(&archive, b"demo contents");
        let error = decompress_demo_with_limit(&archive, 4, || true).unwrap_err();
        assert!(error.to_string().contains("decompressed size limit"));
        assert!(!root.join("match.dem.part").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn decompression_rejects_non_demo_content() {
        let root = temporary_root("decompress-header");
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("match.dem.bz2");
        compressed(&archive, b"not a CS2 demo");
        let error = decompress_demo_with_limit(&archive, MAX_DEMO_BYTES, || true).unwrap_err();
        assert!(error.to_string().contains("CS2 demo header"));
        assert!(!root.join("match.dem.part").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn game_state_blocks_work_during_live_play() {
        let state = GameState::new();
        {
            let mut inner = state.0.lock().unwrap();
            inner.process_running = true;
            inner.process_checked_at = Instant::now();
        }
        state.update(&json!({"map": {"phase": "live"}}));
        assert!(!state.heavy_work_allowed());
        state.update(&json!({"map": {"phase": "gameover"}}));
        assert!(state.heavy_work_allowed());
        assert!(state.snapshot().2.is_some());
    }
}
