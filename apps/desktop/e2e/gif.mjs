// Animated GIFs of the app for the README: frames taken from the page while a scenario drives
// it, a pointer drawn where the clicks go, and keys typed one at a time. Only the pixels that
// change between frames are written, so a mostly still window stays small.
import { writeFileSync } from "node:fs";
import gifenc from "gifenc";
import { PNG } from "pngjs";

const { GIFEncoder, quantize, applyPalette } = gifenc;

/** The pointer, an arrow over the page that moves to each click. */
const POINTER = `(() => {
  if (document.getElementById("e2e-pointer")) return;
  const p = document.createElement("div");
  p.id = "e2e-pointer";
  p.innerHTML = '<svg width="22" height="22" viewBox="0 0 22 22"><path d="M3 2l14 9-6 1.5L8 19z" fill="#fff" stroke="#111" stroke-width="1.5" stroke-linejoin="round"/></svg>';
  p.style.cssText = "position:fixed;left:-40px;top:-40px;z-index:2147483647;pointer-events:none;transition:left .45s ease,top .45s ease,transform .12s";
  document.body.append(p);
})()`;

/** Records the page into a GIF: `start`, drive the app, then `save`. */
export class Recorder {
  /** `scale` shrinks the frames (0.6 of 1600×960 is 960×576); `fps` caps how often one is taken. */
  constructor(page, { scale = 0.6, fps = 12 } = {}) {
    this.page = page;
    this.scale = scale;
    this.interval = 1000 / fps;
    this.frames = [];
    this.running = null;
  }

  async start() {
    await this.page.eval(POINTER);
    this.frames = [];
    this.stopped = false;
    this.running = this.loop();
  }

  async loop() {
    const { width, height } = await this.page.eval(`({ width: innerWidth, height: innerHeight })`);
    const clip = { x: 0, y: 0, width, height, scale: this.scale };
    while (!this.stopped) {
      const at = Date.now();
      const { data } = await this.page.send("Page.captureScreenshot", { format: "png", clip });
      this.frames.push({ at, png: Buffer.from(data, "base64") });
      const left = this.interval - (Date.now() - at);
      if (left > 0) await this.page.sleep(left);
    }
  }

  /** Holds the picture still for `ms`, as a pause for the eye. */
  hold(ms) {
    return this.page.sleep(ms);
  }

  /** Stops and writes the GIF to `path`; the last frame stays `endMs`. Returns its size. */
  async save(path, endMs = 1500) {
    this.stopped = true;
    await this.running;
    const bytes = encode(this.frames, endMs);
    writeFileSync(path, bytes);
    // A few frames as pictures, to look at the GIF without playing it.
    if (process.env.PLATYPUS_GIF_FRAMES) {
      for (let k = 0; k < 6; k++) {
        const frame = this.frames[Math.round((k * (this.frames.length - 1)) / 5)];
        writeFileSync(`${path}.${k}.png`, frame.png);
      }
    }
    return bytes.length;
  }
}

/** Moves the pointer to the centre of `selector` (or to `x`, `y`) and clicks there. */
export async function pointAndClick(page, target, options = {}) {
  const { x, y } = typeof target === "string" ? await center(page, target) : target;
  await page.eval(`(() => { const p = document.getElementById("e2e-pointer"); if (p) { p.style.left = "${x - 3}px"; p.style.top = "${y - 2}px"; } })()`);
  await page.sleep(500);
  await page.eval(`(() => { const p = document.getElementById("e2e-pointer"); if (p) p.style.transform = "scale(.8)"; })()`);
  await page.click(x, y, options);
  await page.sleep(120);
  await page.eval(`(() => { const p = document.getElementById("e2e-pointer"); if (p) p.style.transform = ""; })()`);
}

/** Moves the pointer to `selector` without clicking. */
export async function pointAt(page, target) {
  const { x, y } = typeof target === "string" ? await center(page, target) : target;
  await page.eval(`(() => { const p = document.getElementById("e2e-pointer"); if (p) { p.style.left = "${x - 3}px"; p.style.top = "${y - 2}px"; } })()`);
  await page.sleep(500);
}

async function center(page, selector) {
  const r = await page.rect(selector);
  return { x: Math.round(r.x + r.w / 2), y: Math.round(r.y + r.h / 2) };
}

/** Types `text` one character at a time, as a person would; a newline is Enter. */
export async function typeSlowly(page, text, msPerKey = 45) {
  for (const ch of text) {
    if (ch === "\n") await page.key("Enter", "Enter", 13);
    else await page.type(ch);
    await page.sleep(msPerKey);
  }
}

/** The frames as a GIF with one palette, each frame only where it differs from the last. */
function encode(frames, endMs) {
  const images = frames.map((f) => PNG.sync.read(f.png));
  const { width, height } = images[0];
  // One palette from a sample of the frames, with the last slot left for "unchanged".
  const step = Math.max(1, Math.floor(images.length / 8));
  const sample = Buffer.concat(images.filter((_, i) => i % step === 0).map((img) => img.data));
  const palette = quantize(new Uint8Array(sample.buffer, sample.byteOffset, sample.length), 255);
  const clear = palette.length;
  const fullPalette = [...palette, [255, 0, 255]];
  const gif = GIFEncoder();
  let previous = null;
  let pending = null;
  const flush = (delay) => {
    gif.writeFrame(pending.index, width, height, {
      palette: fullPalette,
      delay,
      transparent: pending.first ? false : true,
      transparentIndex: clear,
      dispose: 1,
    });
  };
  for (let i = 0; i < images.length; i++) {
    const index = applyPalette(new Uint8Array(images[i].data.buffer, images[i].data.byteOffset, images[i].data.length), palette);
    let changed = previous === null;
    const out = previous === null ? index : new Uint8Array(index.length);
    if (previous !== null) {
      for (let p = 0; p < index.length; p++) {
        if (index[p] === previous[p]) out[p] = clear;
        else {
          out[p] = index[p];
          changed = true;
        }
      }
    }
    const at = frames[i].at;
    if (!changed) continue; // the frame before it lasts longer
    if (pending) flush(at - pending.at);
    pending = { index: out, at, first: previous === null };
    previous = index;
  }
  if (pending) flush(endMs);
  gif.finish();
  return gif.bytes();
}
