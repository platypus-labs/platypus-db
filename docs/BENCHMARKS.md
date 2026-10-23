# Benchmarks

Measured on 23 October 2026 with the release build of PlatypusDB 0.1.0.

**Machine:** AMD Ryzen 7 9700X, 32 GB RAM, Windows 11 Pro, WebView2 155. Databases run
locally in Docker (`docker/compose.yml`): PostgreSQL 17, MySQL 8.4, MariaDB 11.4.

macOS and Linux have not been measured yet.

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

## DBeaver

DBeaver was not installed on the test machine, so it has not been measured side by side.
The reference values in the plan (5–15 s start, 500 MB–1 GB at rest, ~120 MB installer)
come from DBeaver's typical footprint as a JVM application and are indicative only.
