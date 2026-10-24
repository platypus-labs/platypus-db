// A minimal Chrome DevTools Protocol client for the app's WebView2 page.

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** CDP modifier bits. */
export const CTRL = 2;
export const SHIFT = 8;
export const ALT = 1;

/** Connects to the first page target on `port`, retrying until the app is up. */
export async function connect(port, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let target;
  while (!target) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      target = targets.find((t) => t.type === "page");
    } catch {
      // Not listening yet.
    }
    if (!target) {
      if (Date.now() > deadline) throw new Error(`no WebView2 page on port ${port}`);
      await sleep(100);
    }
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.onopen = resolve;
    ws.onerror = reject;
  });
  let nextId = 1;
  const pending = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    const waiter = msg.id && pending.get(msg.id);
    if (!waiter) return;
    pending.delete(msg.id);
    if (msg.error) waiter.reject(new Error(JSON.stringify(msg.error)));
    else waiter.resolve(msg.result);
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const id = nextId++;
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });
  return new Page(send, () => ws.close());
}

export class Page {
  constructor(send, close) {
    this.send = send;
    this.close = close;
  }

  sleep(ms) {
    return sleep(ms);
  }

  /** Evaluates `expression` in the page (promises awaited) and returns its JSON value. */
  async eval(expression) {
    const r = await this.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) {
      throw new Error(r.exceptionDetails.exception?.description ?? `eval failed: ${expression}`);
    }
    return r.result.value;
  }

  async waitFor(expression, timeoutMs = 10_000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (await this.eval(expression)) return;
      await sleep(50);
    }
    throw new Error(`timed out waiting for ${expression}`);
  }

  async rect(selector) {
    return this.eval(`(() => {
      const r = document.querySelector(${JSON.stringify(selector)})?.getBoundingClientRect();
      return r && { x: r.x, y: r.y, w: r.width, h: r.height };
    })()`);
  }

  async mouse(type, x, y, { button = "left", clickCount = 1, modifiers = 0 } = {}) {
    await this.send("Input.dispatchMouseEvent", {
      type,
      x,
      y,
      button,
      clickCount,
      modifiers,
      buttons: button === "right" ? 2 : 1,
    });
  }

  async click(x, y, options = {}) {
    await this.mouse("mousePressed", x, y, options);
    await this.mouse("mouseReleased", x, y, options);
  }

  /** Clicks the centre of the first element matching `selector`. */
  async clickSel(selector, options = {}) {
    const r = await this.rect(selector);
    if (!r) throw new Error(`not found: ${selector}`);
    await this.click(r.x + r.w / 2, r.y + r.h / 2, options);
  }

  async type(text) {
    await this.send("Input.insertText", { text });
  }

  /** A key press, e.g. `key("t", "KeyT", 84, CTRL)`. */
  async key(key, code, keyCode, modifiers = 0) {
    for (const type of ["rawKeyDown", "keyUp"]) {
      await this.send("Input.dispatchKeyEvent", { type, key, code, windowsVirtualKeyCode: keyCode, modifiers });
    }
  }

  async screenshot(path) {
    const { writeFileSync } = await import("node:fs");
    const { data } = await this.send("Page.captureScreenshot", { format: "png" });
    writeFileSync(path, Buffer.from(data, "base64"));
  }
}
