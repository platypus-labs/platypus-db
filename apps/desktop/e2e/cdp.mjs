// A minimal Chrome DevTools Protocol client for the app's WebView2 page.

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** CDP modifier bits. */
export const CTRL = 2;
export const SHIFT = 8;
export const ALT = 1;

/** Connects to the first page target on `port` (or the first `pick` accepts: another window
 *  of the app), retrying until there is one. */
export async function connect(port, timeoutMs = 30_000, pick = () => true) {
  const deadline = Date.now() + timeoutMs;
  let target;
  while (!target) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      target = targets.find((t) => t.type === "page" && pick(t));
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
  const page = new Page(send, () => ws.close());
  page.targetId = target.id;
  return page;
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
      buttons: { left: 1, right: 2, middle: 4 }[button],
    });
  }

  async click(x, y, options = {}) {
    await this.mouse("mousePressed", x, y, options);
    await this.mouse("mouseReleased", x, y, options);
  }

  /** Drags with the left button from (x1, y1) to (x2, y2) as a real pointer does: press,
   *  moves with the button held, release (with `modifiers` held). `before` runs before the
   *  release, with the pointer over the target, e.g. to check what the drag shows there. */
  async drag(x1, y1, x2, y2, { steps = 8, modifiers = 0, before } = {}) {
    await this.mouse("mousePressed", x1, y1);
    for (let i = 1; i <= steps; i++) {
      await this.mouse("mouseMoved", x1 + ((x2 - x1) * i) / steps, y1 + ((y2 - y1) * i) / steps);
      await sleep(16);
    }
    if (before) await before();
    await this.mouse("mouseReleased", x2, y2, { modifiers });
  }

  /** The rectangle of the element `expression` evaluates to, or `null`. */
  async rectOf(expression) {
    return this.eval(`(() => {
      const r = (${expression})?.getBoundingClientRect();
      return r ? { x: r.x, y: r.y, w: r.width, h: r.height } : null;
    })()`);
  }

  /** Drags from the element `from` to the element `to` (expressions), at fractions of
   *  their size (`at`: `[0.5, 0.5]` is the centre), with `drag`'s options. */
  async dragEl(from, to, { fromAt = [0.5, 0.5], toAt = [0.5, 0.5], ...options } = {}) {
    const a = await this.rectOf(from);
    const b = await this.rectOf(to);
    if (!a) throw new Error(`not found: ${from}`);
    if (!b) throw new Error(`not found: ${to}`);
    await this.drag(a.x + a.w * fromAt[0], a.y + a.h * fromAt[1], b.x + b.w * toAt[0], b.y + b.h * toAt[1], options);
  }

  /** Clicks the centre of the first element matching `selector`. */
  async clickSel(selector, options = {}) {
    const r = await this.rect(selector);
    if (!r) throw new Error(`not found: ${selector}`);
    await this.click(r.x + r.w / 2, r.y + r.h / 2, options);
  }

  /** Shows a tab of the open Settings dialog, named by its section in English (the language
   *  of the interface does not matter), and waits for its panel. */
  async settingsTab(section) {
    const tab = `document.querySelector('.modal.settings [role="tab"][data-section=${JSON.stringify(section)}]')`;
    await this.waitFor(`${tab} != null`);
    await this.eval(`${tab}.click()`);
    await this.waitFor(`${tab}.getAttribute('aria-selected') === 'true' && document.querySelector('.settings-body')?.getAttribute('aria-labelledby') === ${tab}.id`);
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
