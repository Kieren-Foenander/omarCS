from __future__ import annotations

import bz2
import fcntl
import hashlib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
import zipfile
from io import BytesIO
from urllib.parse import urlparse
from dataclasses import dataclass, field
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Callable

from .config import data_home, state_home


GSI_HOST = "127.0.0.1"
GSI_PORT = 31982
DISCOVERY_INTERVAL = 30.0
POST_MATCH_WINDOW = 10 * 60.0
IDLE_DISCOVERY_INTERVAL = 15 * 60.0
HELPER_VERSION = "1.7.0"
HELPER_URL = (
    "https://github.com/akiver/boiler-writter/releases/download/"
    f"v{HELPER_VERSION}/boiler-writter-linux-{HELPER_VERSION}.zip"
)
HELPER_SHA256 = "f3c85acebb55a8c8eefb1334db4fce2cda397dc50eb8ecdb5664cedbc900f7ff"
REPLAY_URL = re.compile(rb"https?://replay\d+\.valve\.net/730/[^\x00\s\"]+?\.dem\.bz2")
UNSAFE_PHASES = {"warmup", "live", "intermission"}


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def runtime_root() -> Path:
    return state_home() / "omarcs"


def demos_root() -> Path:
    return data_home() / "omarcs/demos"


def helper_root() -> Path:
    return data_home() / "omarcs/boiler-writter"


def state_path() -> Path:
    return runtime_root() / "autofetch.json"


def extract_replay_urls(payload: bytes) -> list[str]:
    found: list[str] = []
    for match in REPLAY_URL.finditer(payload):
        url = match.group().decode("ascii")
        if url not in found:
            found.append(url)
    return found


def load_state() -> dict:
    try:
        return json.loads(state_path().read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return {"knownUrls": [], "queue": []}


def save_state(state: dict) -> None:
    root = runtime_root()
    root.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(prefix="autofetch.", suffix=".json", dir=root)
    try:
        with os.fdopen(handle, "w", encoding="utf-8") as output:
            json.dump(state, output, indent=2)
            output.write("\n")
        os.replace(temporary, state_path())
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def helper_binary() -> Path:
    return helper_root() / "boiler-writter"


def query_recent_urls(timeout: float = 25.0) -> list[str]:
    binary = helper_binary()
    if not binary.exists():
        raise RuntimeError("Match helper is not installed; run `omarcs setup-auto`")
    helper_root().mkdir(parents=True, exist_ok=True)
    handle, output_name = tempfile.mkstemp(prefix="recent.", suffix=".info", dir=helper_root())
    os.close(handle)
    output = Path(output_name)
    try:
        result = subprocess.run(
            [str(binary), output.name],
            cwd=helper_root(),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
        if result.returncode != 0:
            detail = result.stderr.decode(errors="replace").strip().splitlines()
            raise RuntimeError(detail[-1] if detail else f"match helper exited {result.returncode}")
        urls = extract_replay_urls(output.read_bytes())
        if not urls:
            raise RuntimeError("Steam returned no recent Valve match replays")
        return urls
    finally:
        output.unlink(missing_ok=True)


@dataclass
class GameState:
    phase: str = "unknown"
    updated_at: float = 0.0
    gameover_at: float | None = None
    revision: int = 0
    process_checked_at: float = 0.0
    process_running: bool = False
    lock: threading.Lock = field(default_factory=threading.Lock)

    def update(self, payload: dict, now: float | None = None) -> None:
        phase = str(payload.get("map", {}).get("phase", "")).casefold()
        if not phase:
            return
        moment = time.monotonic() if now is None else now
        with self.lock:
            previous = self.phase
            self.phase = phase
            self.updated_at = moment
            if phase == "gameover" and previous != "gameover":
                self.gameover_at = moment
            self.revision += 1

    def snapshot(self) -> tuple[str, float, float | None, int]:
        with self.lock:
            return self.phase, self.updated_at, self.gameover_at, self.revision

    def heavy_work_allowed(self) -> bool:
        phase, updated_at, _, _ = self.snapshot()
        now = time.monotonic()
        if now - self.process_checked_at >= 2.0:
            running = False
            try:
                for entry in os.scandir("/proc"):
                    if not entry.name.isdigit():
                        continue
                    try:
                        if Path(entry.path, "comm").read_text().strip() == "cs2":
                            running = True
                            break
                    except (OSError, UnicodeDecodeError):
                        continue
            except OSError:
                running = True
            self.process_running = running
            self.process_checked_at = now
        if phase in UNSAFE_PHASES:
            return not self.process_running and now - updated_at > 30.0
        if phase == "unknown" and self.process_running:
            return False
        return True


class GSIHandler(BaseHTTPRequestHandler):
    game_state: GameState

    def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler API
        try:
            length = min(int(self.headers.get("Content-Length", "0")), 1024 * 1024)
            payload = json.loads(self.rfile.read(length))
            self.game_state.update(payload)
            self.send_response(204)
            self.end_headers()
        except (ValueError, json.JSONDecodeError):
            self.send_error(400)

    def log_message(self, format: str, *args: object) -> None:
        return


def start_gsi_server(game_state: GameState) -> ThreadingHTTPServer:
    handler = type("OmarCSGSIHandler", (GSIHandler,), {"game_state": game_state})
    server = ThreadingHTTPServer((GSI_HOST, GSI_PORT), handler)
    threading.Thread(target=server.serve_forever, name="omarcs-gsi", daemon=True).start()
    return server


class InterruptedForGame(RuntimeError):
    pass


def download_demo(url: str, allowed: Callable[[], bool]) -> Path:
    root = demos_root()
    root.mkdir(parents=True, exist_ok=True)
    compressed = root / Path(urlparse(url).path).name
    partial = compressed.with_suffix(compressed.suffix + ".part")
    offset = partial.stat().st_size if partial.exists() else 0
    request = urllib.request.Request(url, headers={"User-Agent": "omarCS/0.1"})
    if offset:
        request.add_header("Range", f"bytes={offset}-")
    with urllib.request.urlopen(request, timeout=30) as response:
        if offset and response.status != 206:
            offset = 0
        mode = "ab" if offset else "wb"
        with partial.open(mode) as output:
            while chunk := response.read(1024 * 1024):
                if not allowed():
                    raise InterruptedForGame("download paused because a match started")
                output.write(chunk)
    os.replace(partial, compressed)
    return compressed


def decompress_demo(compressed: Path, allowed: Callable[[], bool]) -> Path:
    demo = compressed.with_suffix("")
    partial = demo.with_suffix(demo.suffix + ".part")
    try:
        with bz2.open(compressed, "rb") as source, partial.open("wb") as output:
            while chunk := source.read(1024 * 1024):
                if not allowed():
                    raise InterruptedForGame("decompression paused because a match started")
                output.write(chunk)
        os.replace(partial, demo)
        return demo
    except BaseException:
        partial.unlink(missing_ok=True)
        raise


def parse_demo(demo: Path, allowed: Callable[[], bool]) -> None:
    command = [sys.executable, "-m", "omarcs.cli", "import", str(demo), "--quiet"]

    def lower_priority() -> None:
        os.nice(15)

    process = subprocess.Popen(command, preexec_fn=lower_priority)
    while process.poll() is None:
        if not allowed():
            process.send_signal(signal.SIGTERM)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            raise InterruptedForGame("parsing stopped because a match started")
        time.sleep(0.25)
    if process.returncode:
        raise RuntimeError(f"parser exited {process.returncode}")


class AutoFetcher:
    def __init__(self, game_state: GameState) -> None:
        self.game_state = game_state
        self.state = load_state()
        self.state.setdefault("knownUrls", [])
        self.state.setdefault("queue", [])

    def record(self, **updates: object) -> None:
        self.state.update(updates)
        self.state["updatedAt"] = utc_now()
        save_state(self.state)

    def discover(self) -> None:
        self.record(status="polling", error="")
        urls = query_recent_urls()
        known = set(self.state["knownUrls"])
        queue = list(self.state["queue"])
        new_urls = [url for url in reversed(urls) if url not in known]
        for url in new_urls:
            if url not in queue:
                queue.append(url)
        self.record(
            status="queued" if queue else "idle",
            knownUrls=list(dict.fromkeys(urls + self.state["knownUrls"]))[:40],
            queue=queue,
            lastPollAt=utc_now(),
        )

    def process_queue(self) -> None:
        while self.state["queue"] and self.game_state.heavy_work_allowed():
            url = self.state["queue"][0]
            name = Path(urlparse(url).path).name
            self.record(status="downloading", current=name)
            compressed = demos_root() / name
            demo = compressed.with_suffix("")
            if not demo.exists():
                if not compressed.exists():
                    compressed = download_demo(url, self.game_state.heavy_work_allowed)
                self.record(status="decompressing", current=name)
                demo = decompress_demo(compressed, self.game_state.heavy_work_allowed)
            self.record(status="parsing", current=demo.name)
            parse_demo(demo, self.game_state.heavy_work_allowed)
            self.state["queue"].pop(0)
            self.record(status="idle", current="", lastImportedAt=utc_now())


def run_daemon() -> int:
    runtime_root().mkdir(parents=True, exist_ok=True)
    lock_file = (runtime_root() / "autofetch.lock").open("w")
    try:
        fcntl.flock(lock_file, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        print("omarCS auto-fetch is already running", file=sys.stderr)
        return 1

    game_state = GameState()
    server = start_gsi_server(game_state)
    fetcher = AutoFetcher(game_state)
    fetcher.record(status="idle", pid=os.getpid(), gsi=f"http://{GSI_HOST}:{GSI_PORT}/gsi")
    next_poll = time.monotonic() + 15
    handled_gameover: float | None = None
    burst_until = 0.0
    recorded_phase = "unknown"
    work_retry_at = 0.0
    work_failures = 0
    try:
        while True:
            now = time.monotonic()
            phase, _, gameover_at, _ = game_state.snapshot()
            if phase != recorded_phase:
                recorded_phase = phase
                if phase in UNSAFE_PHASES:
                    fetcher.record(status="in-game", gamePhase=phase, error="")
                else:
                    fetcher.record(status="waiting" if phase == "gameover" else "idle", gamePhase=phase)
            if gameover_at is not None and gameover_at != handled_gameover:
                handled_gameover = gameover_at
                next_poll = gameover_at + DISCOVERY_INTERVAL
                burst_until = gameover_at + POST_MATCH_WINDOW
            if game_state.heavy_work_allowed():
                try:
                    if now >= work_retry_at:
                        fetcher.process_queue()
                        if not fetcher.state["queue"]:
                            work_failures = 0
                    if now >= next_poll:
                        fetcher.discover()
                        if now >= work_retry_at:
                            fetcher.process_queue()
                            if not fetcher.state["queue"]:
                                work_failures = 0
                        next_poll = now + (DISCOVERY_INTERVAL if now < burst_until else IDLE_DISCOVERY_INTERVAL)
                except InterruptedForGame as error:
                    fetcher.record(status="paused", error=str(error))
                    work_retry_at = now + DISCOVERY_INTERVAL
                except Exception as error:
                    work_failures += 1
                    fetcher.record(status="waiting", error=str(error), lastPollAt=utc_now())
                    retry_delay = DISCOVERY_INTERVAL * (2 ** (work_failures - 1))
                    work_retry_at = now + min(IDLE_DISCOVERY_INTERVAL, retry_delay)
                    next_poll = now + (DISCOVERY_INTERVAL if now < burst_until else IDLE_DISCOVERY_INTERVAL)
            time.sleep(0.5)
    except KeyboardInterrupt:
        return 0
    finally:
        server.shutdown()


def steam_gsi_path() -> Path:
    candidates = (
        data_home() / "Steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg",
        Path.home() / ".steam/steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg",
    )
    for candidate in candidates:
        if candidate.exists():
            return candidate / "gamestate_integration_omarcs.cfg"
    raise RuntimeError("Could not find the local CS2 cfg directory")


def install_helper() -> None:
    root = helper_root()
    root.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(HELPER_URL, timeout=60) as response:
        archive = response.read()
    if hashlib.sha256(archive).hexdigest() != HELPER_SHA256:
        raise RuntimeError("Downloaded match helper failed its SHA-256 check")
    with zipfile.ZipFile(BytesIO(archive)) as bundle:
        members = {Path(name).name: name for name in bundle.namelist()}
        for name in ("boiler-writter", "libsteam_api.so", "steam_appid.txt"):
            source = bundle.open(members[name])
            target = root / name
            with source, target.open("wb") as output:
                shutil.copyfileobj(source, output)
            target.chmod(0o755 if name == "boiler-writter" else 0o644)


def install_text_file(path: Path, content: str) -> None:
    if path.exists() and path.read_text(encoding="utf-8", errors="replace") != content:
        backup = runtime_root() / "backups"
        backup.mkdir(parents=True, exist_ok=True)
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        shutil.copy2(path, backup / f"{path.name}.{stamp}")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def setup_auto(seed: bool = True) -> None:
    install_helper()
    gsi = steam_gsi_path()
    gsi_content = (
        '"omarCS"\n'
        "{\n"
        f'  "uri" "http://{GSI_HOST}:{GSI_PORT}/gsi"\n'
        '  "timeout" "1.0"\n'
        '  "buffer" "0.2"\n'
        '  "throttle" "1.0"\n'
        '  "heartbeat" "10.0"\n'
        '  "data"\n'
        "  {\n"
        '    "provider" "1"\n'
        '    "map" "1"\n'
        "  }\n"
        "}\n"
    )
    install_text_file(gsi, gsi_content)
    state = load_state()
    if seed and not state.get("knownUrls"):
        state["knownUrls"] = query_recent_urls()
        state["queue"] = []
        state["lastPollAt"] = utc_now()
        state["status"] = "idle"
        save_state(state)

    executable = shutil.which("omarcs")
    if not executable:
        raise RuntimeError("Could not find the installed `omarcs` command")
    service_dir = Path.home() / ".config/systemd/user"
    service_dir.mkdir(parents=True, exist_ok=True)
    service_content = (
        "[Unit]\n"
        "Description=omarCS automatic Valve demo fetcher\n"
        "After=network-online.target\n\n"
        "[Service]\n"
        f"ExecStart={executable} auto-run\n"
        "Restart=on-failure\n"
        "RestartSec=10\n"
        "Nice=10\n"
        "IOSchedulingClass=idle\n"
        "CPUWeight=10\n"
        "IOWeight=10\n\n"
        "[Install]\n"
        "WantedBy=default.target\n"
    )
    install_text_file(service_dir / "omarcs-autofetch.service", service_content)
