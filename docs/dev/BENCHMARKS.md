# Benchmarks

Measured on 23 October 2026 with the release build of PlatypusDB 0.1.0.

**Machine:** AMD Ryzen 7 9700X, 32 GB RAM, Windows 11 Pro, WebView2 155. Databases run
locally in Docker (`docker/compose.yml`): PostgreSQL 17, MySQL 8.4, MariaDB 11.4.

Linux was measured on 28 October 2026 (below); macOS has not been measured yet.

## Against the MVP targets

| Metric | Target | Measured | |
|---|---|---|---|
| Start to interactive window | < 500 ms | 430–500 ms (580 ms on the first start after install) | ✓ |
| Memory at rest, 1 connection open | < 150 MB | 126–132 MB private (app 7 MB + WebView2) | ✓ |
| First rows on screen after the server answers | < 100 ms | 3–9 ms for `SELECT * FROM demo.orders` | ✓ |
| Scrolling 1M rows | 60 fps, no freezes | 178 fps, p95 frame 5.6 ms, no frame over 33 ms | ✓ |
| Installer size | < 20 MB | 5.4 MB (NSIS), 7.6 MB (MSI) | ✓ |

## Streaming

| Workload | Time | Peak memory |
|---|---|---|
| 1M rows PostgreSQL → CSV (CLI) | 1.2 s | 9.5 MB |
| 5M rows PostgreSQL → Parquet, zstd (CLI) | 3.2 s | 31 MB |
| 5M rows PostgreSQL → CSV (CLI) | 3.1 s | 11 MB |
| Fetch all 1M rows into the grid (app) | < 1 s | 345 MB private for the whole app |

The 5M-row query has five columns (`bigint`, `text`, `numeric`, `timestamptz`, `boolean`);
the Parquet file is 45 MB, the CSV 374 MB.

## How the numbers were taken

- **Start:** time from launching `platypus-db.exe` until the connection tree is rendered,
  read through the WebView2 DevTools protocol; five runs.
- **Memory:** sum of the private bytes of `platypus-db.exe` and its WebView2 child processes,
  3 s after start with a connection open. Working set is higher (about 345 MB) because it
  counts pages shared with other WebView2 and Edge processes.
- **Scrolling:** `requestAnimationFrame` timings while scrolling 2,400 px per frame through
  a 1M-row result for 3 s.
- **Streaming:** `platypus query --stats` with `-f csv` or `-f parquet`, peak working set
  sampled every 100 ms.

The GPU runs inside the WebView2 browser process (`--in-process-gpu`): one process fewer and
about 55 MB less at rest, with the same scrolling performance as a separate GPU process.

## Windows, 13 November 2026: PlatypusDB 0.6.1 and DBeaver

Both clients measured back to back on the same machine (Ryzen 7 9700X, 32 GB, Windows 11 Pro),
Docker stopped, five runs each with `scripts/bench-windows.ps1`, no connection open. PlatypusDB
is the installed 0.6.1 release; DBeaver is Community Edition 26.2.3, the Windows zip, with the
Java 25 runtime it bundles and its default `-Xmx1024m`.

| | PlatypusDB 0.6.1 | DBeaver CE 26.2.3 | |
|---|---|---|---|
| Start, warm (median of runs 2-5) | 0.72 s (611-777 ms) | 5.2 s (5,151-5,475 ms) | 7.2× faster |
| Start, first run | 0.90 s | 5.2 s | |
| Private memory at rest (median of 5) | 170 MB (165.6-171.7) | 488 MB (485.2-498.1) | 2.9× less |
| Processes | 6 (app + WebView2) | 7 | |

How the script counts:

- **Start** runs from the launch to the first frame after which the main window (the largest
  visible one, at least 640×400) stays the same for 1 s, captured every 100 ms. "The same" allows
  0.5 % of an 80×50 grid of the window's average brightness to change, so a blinking cursor or a
  spinner in a corner does not keep the count running; a window that never settles stops at 60 s.
  This replaces the exact-pixel comparison of the 0.6.0 run below, which some clients never
  satisfied. On PlatypusDB the two methods give close numbers (an earlier exact-pixel run of 0.6.1:
  742.5 ms, 168.7 MB).
- **Memory** is the private bytes of the whole process tree, 5 s after the window settles. It is
  the moment of the start, not a steady state: a JVM's heap tends to grow with use, so DBeaver's
  number is a lower bound.
- Each client gets a profile of its own, and the script waits for the CPU to fall under 15 %
  before each run.
- **DBeaver's profile is prepared once**: on a fresh profile it opens only its first-run setup
  window and never the main one, so the setup was completed in a profile kept apart (workspace,
  configuration, `user.home` and `APPDATA`, passed with `-DbeaverProfile`) and every run starts
  from it, as a user who has already configured it would. Started with `-nosplash`; the splash
  screen is not counted as the window.

## Windows, 13 November 2026 (0.6.0)

Release build of PlatypusDB 0.6.0, same machine (Ryzen 7 9700X, 32 GB, Windows 11 Pro). These
are the numbers the README and the site show.

- **Start and memory at rest** (`scripts/bench-windows.ps1 -Kind platypus`, five runs, no
  connection open): start until the window has finished drawing (its pixels unchanged for 1 s)
  707-739 ms warm, median 723 ms, 834 ms on the first start; private memory of the app and
  WebView2 164-171 MB, median 170 MB. This method counts until the window stops changing, so it
  is not comparable with the 430-500 ms of 0.1.0 above, taken when the connection tree was
  rendered.
- **A million rows** (`perf-ok-1m-rows-sqlite.mjs`, three runs): first page 274-287 ms, every
  row loaded 326-331 ms later with the page never lagging more than 1 ms, copy with headers of
  all of them 2.8 s (72 MB).
- **Sizes**: Windows installer 20.1 MiB (NSIS), Linux `.deb` 29.7 MiB, 68 MiB installed
  (`Installed-Size` 69,502 KiB, plus the system's WebKitGTK).

The memory is still over the MVP target of < 150 MB, as in 0.3.0 below.

## Windows, 2 November 2026 (group S)

Release build of PlatypusDB 0.3.0 on the same machine as above (Ryzen 7 9700X, 32 GB, Windows
11 Pro; WebView2 is now 156). The scenarios are `apps/desktop/e2e/scenarios/perf-*.mjs`; each
prints its numbers and asserts bounds with a wide margin.

**Memory at rest, 1 connection open** (SQLite, 3 s after expanding it; three runs): 167-168 MB
private (app 10 MB + WebView2 158 MB). The MVP target is < 150 MB and 0.1.0 measured 126-132 MB:
this build is over the target. The start time was not re-measured.

**Long session** (`perf-soak.mjs`, SQLite, no Docker): cycles of open an editor, run and scroll
100,000 rows, chart them, open a table's data, edit a cell and discard, close every tab, open
and close Settings. The private memory of the app and WebView2 is sampled after each cycle
(`target/perf/soak-<time>.csv`, with the JS heap and the DOM node count).

| Run | Cycles | Warm-up level | End | Slope after warm-up |
|---|---|---|---|---|
| 60 minutes | 1,793 | 504 MB | 525 MB (+21 MB) | 32 MB/hour over 45 min |
| 4 minutes (default), twice | 118 | 417-443 MB | 458-461 MB (+18, +41 MB) | not checked under 10 min |

Memory climbs from 165 MB to about 500 MB in the first ten minutes (WebView2's allocator and
the 100,000-row buffers), then stays flat: no leak was found. The JS heap stays at 20-50 MB and
the DOM at 260 nodes throughout. The checks: the end is at most 150 MB above the warm-up level
(the first quarter, at least 90 s) and, for runs of 10 minutes or more, the slope is at most
60 MB/hour. The app process alternates between 15 and 50 MB as a result is freed and the next
is loaded.

**Slow remote database** (`perf-slow-remote.mjs`, a node TCP proxy adding 150 ms per chunk each
way and capping the link at 1 MB/s, PostgreSQL in Docker behind it). Two findings, both fixed:
a server that accepts the connection and never answers made the connect hang for good (`connect_timeout`
only bounds the TCP connect), and the `connect_timeout` option of a saved connection never reached
the driver (it waited the default 10 s whatever the option said).

| Step | Measured | Bound |
|---|---|---|
| Silent server, default timeout | gives up after 10.1 s, "did not answer within 10 s" | 9-12.5 s |
| Silent server, `connect_timeout = 3` option | 3.1 s, "did not answer within 3 s" | 2.5-6 s |
| Connect through the slow link | 6.4 s (password dialog, connect, completion metadata) | < 15 s |
| Page lag / longest frame while connecting or loading | 0-1 ms / 6-11 ms | < 200 ms / < 250 ms |
| Cancel of `pg_sleep(60)`, once running on the server | 0.68 s | < 2.5 s |
| 50,000 rows (4 columns) over the link | 37.4 s (500-row fetches, one round trip each) | < 90 s |

While the 50,000 rows arrive the page answers within 2-5 ms and draws 6,700 frames (longest 10 ms).
A Cancel clicked at once, before the statement reaches the server over this link, is lost (the
statement runs to its end and Cancel stays enabled); a second click works. Not fixed: see the
`BUG` note in the scenario.

**Millions of rows** (`perf-millions-postgres.mjs`, `s_perf_5m`: 5,000,000 rows, 6 columns, primary
key on `id`), table data opened from the navigator and as queries; memory is the private memory of
the app plus WebView2:

| Step | Measured | Bound |
|---|---|---|
| First rows on screen (table data) | 170 ms | < 1.5 s |
| Count rows | 60-130 ms | < 2 s |
| Scrolling the streamed table / query | 180 fps, longest frame 6-9 ms | >= 40 fps, < 250 ms |
| "Fetch all" of the 5M rows | 7.5 s | < 20 s |
| Sort by the key from the header, both ways | 115-140 ms | < 1 s |
| Filter `grp = …` from the grid menu (5,000 rows, all fetched) | 500 ms | < 2 s |
| Query `SELECT *`, first rows | 120 ms | < 1 s |
| Query ordered by an unindexed column (server sorts 5M rows) | 1.9 s | < 6 s |
| Cancel of a 5M x 5M x 5M cross join | 62-64 ms | < 1.5 s |
| Memory: idle / table opened / 5M rows buffered / at the end | 177 / 198 / 556 / 300 MB | < 1200 MB buffered, < 800 MB end |

The 5M rows buffered cost 280 MB in the app process (the backend holds the result; the grid keeps
100 blocks of 500 rows). The grid caps its scroll height at 10,000,000 px and maps it to the row,
so the last row is reached with Ctrl+End.

**Thousands of objects** (`perf-objects-postgres.mjs`: schema with 5,000 tables chained by
foreign keys, 500 views, 200 functions):

| Step | Measured | Bound |
|---|---|---|
| Expand the schema (5,000 tables listed) / each folder | 70-115 ms / 20-45 ms | < 500 ms |
| Navigator filter to one table | 7 ms | < 500 ms |
| Completion of a table name after `schema.` (32 options) | 380-390 ms | < 1.5 s |
| Metadata search, narrow / 1,000 hits shown | 61 ms / 105-115 ms | < 500 ms / < 1 s |
| One table's object page (from the menu) | 260 ms | < 1 s |
| ER diagram of 100 tables, 99 foreign keys | 0.91-0.96 s | < 3 s |
| Memory: connected / schema expanded / end | 180 / 242 / 431 MB | < 900 MB at the end |

The navigator keeps fewer than 1,500 rows in the DOM with 5,700 objects open.

## Linux

Release build of 28 October 2026 on Ubuntu 24.04 (WebKitGTK 2.52), in a 4-vCPU cloud
container: an X server with no GPU (Xvfb, software rendering) and the dev databases running
beside it. It shows that the app builds, starts and works on Linux; a desktop with a GPU will
do better, and has yet to be measured.

| Metric | Target | Measured |
|---|---|---|
| Start to interactive window | < 500 ms | 1.3–1.5 s (first paint 0.7 s) |
| Memory at rest, 1 connection open | < 150 MB | 300 MB private (app 65 MB, web process 230 MB) |
| Scrolling 1M rows | 60 fps, no freezes | Scrolls and loads rows as it goes, no freeze; frame rate not meaningful without a GPU |

Most of the web process's memory is the software renderer's buffers: with
`WEBKIT_DISABLE_COMPOSITING_MODE=1` the total falls to 180 MB, at no cost to the start.
Checked by hand with a window manager (openbox): the app's own title bar moves, maximizes
(also with a double-click, fixed for Linux), restores, minimizes and closes the window, and
the edges resize it.

`scripts/bench.py` takes these numbers (start: five runs, until the screen settles;
memory: the private memory of the app and its WebKit processes) and the CI job `bench`
compares them with `scripts/bench-thresholds.json`.

## Scale

`crates/platypus-conformance/tests/scale.rs`, against the dev databases, prints its
timings with `cargo test -p platypus-conformance --test scale -- --nocapture`:

Debug build of the tests, same container as above:

| | PostgreSQL | MySQL | MariaDB | SQL Server | SQLite | DuckDB |
|---|---|---|---|---|---|---|
| Tables in the schema | 10,000 | 2,000 | 2,000 | 10,000 | 10,000 | 10,000 |
| Navigator: list the objects | 0.22 s | 0.04 s ¹ | 0.09 s | 0.78 s | 0.04 s | 0.10 s |
| Completion: every column | 0.13 s | 0.05 s | 0.05 s | 0.59 s | 0.28 s | 0.18 s |
| One table's details | 0.02 s | 0.01 s | 0.19 s | 0.35 s | 0.06 s | 0.25 s |
| Metadata search | 0.12 s | 0.04 s | 0.04 s | 0.89 s | 0.09 s | 0.08 s |
| 1M rows to CSV, NDJSON and Parquet at once | 14 s | 17 s | 17 s | 17 s | 11 s | 11 s |
| 32 MB text + 16 MB binary cell: read | 0.44 s | 0.37 s | 0.30 s | 0.84 s ² | 0.38 s | 0.32 s |

MySQL and MariaDB get 2,000 tables because creating one takes them ~5 ms
(`PLATYPUS_SCALE_TABLES` sets the count for every database). Exporting the large cells to
CSV, JSON, SQL, XLSX and Parquet takes 14–15 s on every database in the debug build.

¹ After the first listing: MySQL computes the statistics (rows, size) of new tables when
they are first listed, about 0.5 ms a table (2.1 s for these 2,000), and keeps them for a
day (`information_schema_stats_expiry`).
² 10.5 s before tiberius read `max` values a chunk at a time and copied packets whole,
which also speeds up every SQL Server result.

In the app, the navigator renders only the rows around its view: with 10,000 tables
expanding the connection took 0.2 s instead of 3.7 s and selecting a row 0.07 s instead of
0.5 s (Chromium, the app's frontend on a fake backend); completion lists the tables among
10,000 in about 0.1 s.

## Side by side

Sizes of the Linux downloads, measured on 29 October 2026 from the official release files.
The start and the memory in this table are Linux ones and only PlatypusDB's were measured: running
the others was not possible in the test container, so they are left blank rather than guessed.
DBeaver's start and memory on Windows are in "Windows, 13 November 2026: PlatypusDB 0.6.1 and
DBeaver" above.

| | PlatypusDB 0.2.0 | DBeaver CE 26.2.3 | DataGrip 2026.2.6 | TablePlus 2.2.0 | Beekeeper Studio 6.2.0 |
|---|---|---|---|---|---|
| Runtime | Rust, the system's web view | Java (Eclipse RCP), JRE bundled | Java (IntelliJ platform), JetBrains Runtime bundled | Native, GTK 3 | Electron (Chromium and Node bundled) |
| Download | 19.5 MiB (`.deb`) | 122 MiB (`.tar.gz`) | 1,047 MiB (`.tar.gz`) | 60 MiB (`.deb`) | 346 MiB (`.AppImage`) |
| On disk | 46 MiB (+ WebKitGTK, shared with the system) | 206 MiB, of which the JRE 106 MiB | not measured | 180 MiB (`Installed-Size` of the `.deb`, + GTK libraries) | 1,105 MiB |
| Start to interactive window | 1.4 s (Xvfb, no GPU) | not measured | not measured | not measured | not measured |
| Memory at rest, 1 connection | 310 MB private (Xvfb, no GPU) | not measured; heap up to 1 GB (`-Xmx1024m` in `dbeaver.ini`) | not measured | not measured | not measured |
| License | AGPL-3.0 | Apache 2.0 | Proprietary | Proprietary | GPLv3 (Community Edition) |

DataGrip's size is the one JetBrains' release service gives for the Linux archive
(`data.services.jetbrains.com/products/releases?code=DG`); TablePlus's are the `Size` and
`Installed-Size` of its Debian package (`deb.tableplus.com`, Ubuntu 24 repository). Both on
29 October 2026.

PlatypusDB's numbers are the Linux ones above (software rendering, no GPU); on Windows 0.6.1
starts in 0.72 s and keeps 170 MB, against DBeaver's 5.2 s and 488 MB on the same machine. On
Linux the other clients can be measured with `scripts/bench.py`'s method:
`python3 scripts/bench.py other <name> <command>` starts a client five times, with a profile
in a temporary HOME, until the screen settles (watched for up to 60 s), and reads its private
memory at rest, without a connection. On 29 October 2026 the cloud container could not
download DBeaver, DataGrip, TablePlus or Beekeeper Studio (their hosts are outside its
network policy), so the rows above are still blank.
