"""Benchmarks with thresholds, to stop regressions: decoding (the criterion benchmarks of
platypus-driver-pg), and the start and memory of the release build on Linux.

    cargo bench -p platypus-driver-pg --bench decode
    python3 scripts/bench.py decode

    npm --prefix apps/desktop run tauri build -- --no-bundle
    xvfb-run -a -s "-screen 0 1600x1000x24" python3 scripts/bench.py app target/release/platypus-db

`app` needs an X display, Pillow (`pip install pillow`) and xdotool. It times the start from
launching the app to the last change on screen, the connection listed in the navigator
(five runs), then sums the private memory of the app and its web view processes with a
SQLite connection open. The limits are in scripts/bench-thresholds.json: a few times what
the dev machines measure, so a run fails on a regression, not on a slow runner.
Python, not Node like the other scripts: it reads the screen and /proc.
"""

import json
import os
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
    from PIL import ImageGrab

    return ImageGrab.grab(xdisplay=os.environ.get("DISPLAY"))


def same(a, b):
    """Whether two screens differ by fewer than 0.1% of their pixels."""
    from PIL import ImageChops

    diff = ImageChops.difference(a.convert("L"), b.convert("L")).point(lambda v: 255 if v > 16 else 0)
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


def launch(exe, profile):
    """Starts the app; returns it and the seconds until the screen settled."""
    t0 = time.perf_counter()
    app = subprocess.Popen(
        [exe],
        env=dict(os.environ, PLATYPUS_DATA_DIR=str(profile)),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    frames = []
    while time.perf_counter() - t0 < 8:
        frames.append((time.perf_counter() - t0, grab()))
        # Leaves the X server and the app the CPU between grabs.
        time.sleep(0.02)
    final = frames[-1][1]
    return app, next(t for t, frame in frames if same(frame, final))


def stop(app):
    app.terminate()
    app.wait()


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
        starts = []
        for run in range(START_RUNS):
            app, ready = launch(exe, profile)
            starts.append(ready * 1000)
            print(f"start {run + 1}: {starts[-1]:.0f} ms")
            # The last run stays open for the memory.
            if run < START_RUNS - 1:
                stop(app)
                time.sleep(1)
        # Expands the connection, the first row of the navigator: it connects.
        window = subprocess.run(
            ["xdotool", "search", "--sync", "--pid", str(app.pid)], capture_output=True, text=True, check=True
        ).stdout.split()[-1]
        subprocess.run(["xdotool", "mousemove", "--window", window, "67", "124", "click", "1"], check=True)
        time.sleep(3)
        memory = private_mb(app.pid)
        stop(app)

    start = sorted(starts)[len(starts) // 2]
    print(f"start to interactive window: median {start:.0f} ms (limit {THRESHOLDS['start_ms']})")
    print(f"memory with a connection open: {memory} MB private (limit {THRESHOLDS['memory_mb']})")
    failed = []
    if start > THRESHOLDS["start_ms"]:
        failed.append(f"start: {start:.0f} ms, over {THRESHOLDS['start_ms']}")
    if memory > THRESHOLDS["memory_mb"]:
        failed.append(f"memory: {memory} MB, over {THRESHOLDS['memory_mb']}")
    return failed


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in ("decode", "app") or (sys.argv[1] == "app" and len(sys.argv) < 3):
        sys.exit(__doc__)
    failed = decode() if sys.argv[1] == "decode" else app_bench(sys.argv[2])
    for line in failed:
        print(f"FAIL {line}", file=sys.stderr)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
