// Renders the loading animation (docs/brand/loading/loader.html) to an animated PNG with a
// transparent background, for the places that cannot run its CSS: README files, documents,
// chats. Each frame is a headless screenshot of the page stopped at that time.
//
//   node scripts/loading-animation.mjs            # docs/brand/loading/loading-animated.png
//
// Uses a Chromium-based browser (Chrome, Edge or Chromium; BROWSER to choose one) and pngjs from
// the desktop app's npm packages (npm --prefix apps/desktop ci).
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { crc32, deflateSync } from "node:zlib";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const { PNG } = createRequire(join(root, "apps/desktop/package.json"))("pngjs");

const SIZE = 256; // the canvas; the head is 232 px, room for the bounce
const FPS = 50;
const CYCLE_MS = 1600;
const page = pathToFileURL(join(root, "docs/brand/loading/loader.html")).href;
const output = join(root, "docs/brand/loading/loading-animated.png");

const browser = [
  process.env.BROWSER,
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
].find((path) => path && existsSync(path));
if (!browser) {
  console.error("No Chromium-based browser found: set BROWSER to its executable.");
  process.exit(1);
}

// A profile of its own, as in social-preview.mjs: a headless run on an open profile exits at once.
const scratch = mkdtempSync(join(tmpdir(), "loading-animation-"));
const frames = [];
try {
  const count = (CYCLE_MS * FPS) / 1000;
  for (let i = 0; i < count; i++) {
    const shot = join(scratch, `${i}.png`);
    execFileSync(browser, [
      `--user-data-dir=${join(scratch, "profile")}`,
      "--headless",
      "--disable-gpu",
      "--hide-scrollbars",
      "--force-device-scale-factor=1",
      "--default-background-color=00000000",
      `--window-size=${SIZE},${SIZE}`,
      "--virtual-time-budget=2000",
      `--screenshot=${shot}`,
      `${page}?t=${(i * 1000) / FPS}&size=232`,
    ], { stdio: "ignore" });
    frames.push(crop(PNG.sync.read(readFileSync(shot)), SIZE));
    process.stdout.write(`\rframe ${i + 1}/${count}`);
  }
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
writeFileSync(output, apng(frames, FPS));
console.log(`\nWrote docs/brand/loading/loading-animated.png (${frames.length} frames)`);

/** The top left size×size of a screenshot: headless windows have a minimum width, larger than
 *  the frame, so the page draws the head at the top left. */
function crop(image, size) {
  const out = new PNG({ width: size, height: size });
  PNG.bitblt(image, out, 0, 0, size, size, 0, 0);
  return out;
}

/** An APNG that loops forever; each frame replaces the one before (no blending). */
function apng(images, fps) {
  const { width, height } = images[0];
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0);
    body.copy(out, 4);
    out.writeUInt32BE(crc32(body), body.length + 4);
    return out;
  };
  const u32 = (...values) => {
    const buffer = Buffer.alloc(values.length * 4);
    values.forEach((value, i) => buffer.writeUInt32BE(value, i * 4));
    return buffer;
  };
  // Scanlines with filter 0 (none), RGBA.
  const pixels = (image) => {
    const row = width * 4;
    const raw = Buffer.alloc((row + 1) * height);
    for (let y = 0; y < height; y++) image.data.copy(raw, y * (row + 1) + 1, y * row, (y + 1) * row);
    return deflateSync(raw, { level: 9 });
  };
  const ihdr = Buffer.concat([u32(width, height), Buffer.from([8, 6, 0, 0, 0])]);
  const parts = [Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("acTL", u32(images.length, 0))];
  let sequence = 0;
  images.forEach((image, i) => {
    // fcTL: sequence, size, offset, delay 1/fps, dispose none, blend source.
    const delay = Buffer.from([0, 1, fps >> 8, fps & 255, 0, 0]);
    parts.push(chunk("fcTL", Buffer.concat([u32(sequence++, width, height, 0, 0), delay])));
    const data = pixels(image);
    parts.push(i === 0 ? chunk("IDAT", data) : chunk("fdAT", Buffer.concat([u32(sequence++), data])));
  });
  parts.push(chunk("IEND", Buffer.alloc(0)));
  return Buffer.concat(parts);
}
