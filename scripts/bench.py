"""Benchmarks with thresholds, to stop regressions: decoding (the criterion benchmarks of
platypus-driver-pg), and the start and memory of the release build on Linux.

    cargo bench -p platypus-driver-pg --bench decode
    python3 scripts/bench.py decode

    npm --prefix apps/desktop run tauri build -- --no-bundle
    xvfb-run -a -s "-screen 0 1600x1000x24" python3 scripts/bench.py app target/release/platypus-db

    xvfb-run -a -s "-screen 0 1600x1000x24" python3 scripts/bench.py other dbeaver ~/dbeaver/dbeaver

`app` needs an X display, Pillow (`pip install pillow`) and xdotool. It times the start from
launching the app to the last change on screen, the connection listed in the navigator
(five runs), then sums the private memory of the app and its web view processes with a
SQLite connection open. The limits are in scripts/bench-thresholds.json: a few times what
the dev machines measure, so a run fails on a regression, not on a slow runner.
`other` measures another client (DBeaver, Beekeeper Studio, ...) the same way, for the side by
side table of docs/BENCHMARKS.md: five starts with a profile of its own in a temporary HOME,
then its memory at rest, without a connection. It prints the numbers and has no limits.
Python, not Node like the other scripts: it reads the screen and /proc.
"""

import json
import os
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
THRESHOLDS = json.loads((ROOT / "scripts" / "bench-thresholds.json").read_text())
# Rows each decode benchmark decodes per iteration (`ROWS` in benches/decode.rs).
DECODE_ROWS = 100_000
# Starts of the app timed; the median counts.
START_RUNS = 5
# Seconds the screen is watched after a launch: our app settles in about 1.5 s, a JVM client
# like DBeaver or DataGrip takes many times that.
APP_WATCH_S = 8
OTHER_WATCH_S = 60


def decode():
    """Million rows per second of each decode benchmark, against its floor."""
    failed = []
    for name, floor in THRESHOLDS["decode_mrows_per_s"].items():
        estimates = ROOT / "target" / "criterion" / "decode" / name / "new" / "estimates.json"
        if not estimates.exists():
            failed.append(f"decode/{name}: no result in {estimates}")
            continue
        ns = json.loads(estimates.read_text())["mean"]["point_estimate"]
        rate = DECODE_ROWS / ns * 1e3
        print(f"decode/{name}: {rate:.1f} M rows/s (floor {floor})")
        if rate < floor:
            failed.append(f"decode/{name}: {rate:.1f} M rows/s, under {floor}")
    return failed


def grab():
    """The screen, in grey and a quarter of its size: enough to tell frames apart, and
    seconds of them fit in a few MB."""
    from PIL import ImageGrab

    screen = ImageGrab.grab(xdisplay=os.environ.get("DISPLAY"))
    return screen.convert("L").reduce(4)


def same(a, b):
    """Whether two screens differ by fewer than 0.1% of their pixels."""
    from PIL import ImageChops

    diff = ImageChops.difference(a, b).point(lambda v: 255 if v > 16 else 0)
    return diff.histogram()[255] <= 0.001 * a.width * a.height


def processes(pid):
    """`pid` and its descendants: the app and its web view processes."""
    children = {}
    for entry in os.listdir("/proc"):
        if entry.isdigit():
            try:
                stat = Path(f"/proc/{entry}/stat").read_text()
            except OSError:
                continue
            children.setdefault(int(stat.rsplit(")", 1)[1].split()[1]), []).append(int(entry))
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        out.append(p)
        todo += children.get(p, [])
    return out


def private_mb(pid):
    total = 0
    for p in processes(pid):
        for line in Path(f"/proc/{p}/smaps_rollup").read_text().splitlines():
            key, value = line.split(":", 1)
            if key in ("Private_Clean", "Private_Dirty"):
                total += int(value.split()[0])
    return total // 1024


class NoWindow(Exception):
    pass


def launch(command, env, watch_s):
    """Starts `command`; returns it and the seconds until the screen settled. Raises
    `NoWindow` when it exits or draws nothing."""
    t0 = time.perf_counter()
    app = subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    blank = grab()
    frames = []
    while time.perf_counter() - t0 < watch_s:
        frames.append((time.perf_counter() - t0, grab()))
        # Leaves the X server and the app the CPU between grabs.
        time.sleep(0.02)
    final = frames[-1][1]
    if app.poll() is not None or same(final, blank):
        stop(app)
        raise NoWindow(f"{command[0]} exited or showed no window (exit code {app.returncode})")
    return app, next(t for t, frame in frames if same(frame, final))


def stop(app):
    """Ends the app and the processes it started, so that the next start begins from nothing."""
    for pid in processes(app.pid)[1:]:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    app.terminate()
    app.wait()


def time_starts(command, env, watch_s):
    """Five timed starts, `START_RUNS`; returns the still running last one and the median
    start in ms."""
    starts = []
    for run in range(START_RUNS):
        app, ready = launch(command, env, watch_s)
        starts.append(ready * 1000)
        print(f"start {run + 1}: {starts[-1]:.0f} ms")
        # The last run stays open for the memory.
        if run < START_RUNS - 1:
            stop(app)
            time.sleep(1)
    return app, sorted(starts)[len(starts) // 2]


def app_bench(exe):
    with tempfile.TemporaryDirectory() as tmp:
        profile = Path(tmp)
        db = sqlite3.connect(profile / "bench.db")
        db.execute("CREATE TABLE t (id INTEGER PRIMARY KEY, label TEXT)")
        db.commit()
        db.close()
        (profile / "connections.toml").write_text(
            'version = 6\n\n[[connections]]\nid = "00000000-0000-4000-8000-0000000b0001"\n'
            f'name = "Bench"\ndriver = "sqlite"\ndatabase = \'{profile / "bench.db"}\'\n'
            "save_password = false\n"
        )
        env = dict(os.environ, PLATYPUS_DATA_DIR=str(profile))
        app, start = time_starts([exe], env, APP_WATCH_S)
        # Expands the connection, the first row of the navigator: it connects.
        window = subprocess.run(
            ["xdotool", "search", "--sync", "--pid", str(app.pid)],
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        ).stdout.split()[-1]
        subprocess.run(["xdotool", "mousemove", "--window", window, "67", "124", "click", "1"], check=True)
        time.sleep(3)
        memory = private_mb(app.pid)
        stop(app)

    print(f"start to interactive window: median {start:.0f} ms (limit {THRESHOLDS['start_ms']})")
    print(f"memory with a connection open: {memory} MB private (limit {THRESHOLDS['memory_mb']})")
    failed = []
    if start > THRESHOLDS["start_ms"]:
        failed.append(f"start: {start:.0f} ms, over {THRESHOLDS['start_ms']}")
    if memory > THRESHOLDS["memory_mb"]:
        failed.append(f"memory: {memory} MB, over {THRESHOLDS['memory_mb']}")
    return failed


def other_bench(name, command):
    """Start and memory at rest of another client, for the side by side table. Its profile
    lives in a temporary HOME, the same for the five starts: the first may show a welcome
    page, the median does not."""
    # The memory counts the processes a launcher starts; a launcher that exits once the client
    # is up reads as a failed start.
    with tempfile.TemporaryDirectory() as home:
        app, start = time_starts(command, dict(os.environ, HOME=home), OTHER_WATCH_S)
        time.sleep(3)
        memory = private_mb(app.pid)
        stop(app)
    print(f"{name}: start to settled window: median {start:.0f} ms")
    print(f"{name}: memory at rest, no connection: {memory} MB private")
    return []


def main():
    modes = {"decode": 2, "app": 3, "other": 4}
    if len(sys.argv) < 2 or len(sys.argv) < modes.get(sys.argv[1], 99):
        sys.exit(__doc__)
    try:
        if sys.argv[1] == "decode":
            failed = decode()
        elif sys.argv[1] == "app":
            failed = app_bench(sys.argv[2])
        else:
            failed = other_bench(sys.argv[2], sys.argv[3:])
    except NoWindow as e:
        failed = [str(e)]
    for line in failed:
        print(f"FAIL {line}", file=sys.stderr)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
