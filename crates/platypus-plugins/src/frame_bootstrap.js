(() => {
  const app = window.parent;
  const waiting = new Map();
  const listeners = new Set();
  let next = 1;
  let init = null;
  const send = (message) => app.postMessage({ platypus: 1, ...message }, "*");
  window.addEventListener("message", (e) => {
    const m = e.data;
    if (e.source !== app || !m || m.platypus !== 1) return;
    if (m.type === "ping") send({ type: "pong", n: m.n });
    else if (m.type === "answer") {
      const call = waiting.get(m.id);
      if (!call) return;
      waiting.delete(m.id);
      if (typeof m.error === "string") call.reject(new Error(m.error));
      else call.resolve(m.value === undefined ? null : JSON.parse(m.value));
    } else if (m.type === "event") {
      const data = JSON.parse(m.data);
      if (m.event === "init") init = data;
      for (const listener of listeners) listener(m.event, data);
    }
  });
  window.platypus = Object.freeze({
    request: (name, params = {}) =>
      new Promise((resolve, reject) => {
        const id = next++;
        waiting.set(id, { resolve, reject });
        send({ type: "request", id, name: String(name), params: JSON.stringify(params) });
      }),
    on: (listener) => {
      listeners.add(listener);
      // A page that listens late still hears where it is.
      if (init) listener("init", init);
      return () => listeners.delete(listener);
    },
  });
})();
