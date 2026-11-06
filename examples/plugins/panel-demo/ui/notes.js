// The app gives this page `window.platypus`: `request(name, params)` answers by a promise, and
// `on(listener)` tells of `init` (where it is) and `theme`. There is nothing else to reach it with.
const element = (id) => document.getElementById(id);
const show = (text) => {
  element("result").textContent = text;
};

function applyTheme(theme) {
  for (const [name, value] of Object.entries(theme)) document.documentElement.style.setProperty(name, value);
}

platypus.on((event, data) => {
  if (event === "theme") applyTheme(data);
  if (event === "init") {
    applyTheme(data.theme);
    element("where").textContent = data.connection ? `On ${data.connection.name} (${data.connection.driver})` : "On no connection";
  }
});

platypus.request("setting", { key: "greeting" }).then((text) => {
  element("greeting").textContent = text;
});

platypus.request("state_get").then((saved) => {
  element("text").value = saved?.text ?? "";
});

element("save").onclick = () =>
  platypus.request("state_set", { value: { text: element("text").value } }).then(
    () => show("Saved"),
    (e) => show(e.message),
  );

element("count").onclick = () =>
  platypus.request("run_action", { action: "count-words", context: { text: element("text").value } }).then(
    (outcome) => show(outcome.text),
    (e) => show(e.message),
  );

element("query").onclick = () => platypus.request("open_query", { text: element("text").value }).catch((e) => show(e.message));

element("toast").onclick = () => platypus.request("notify", { text: "Hello from the notes" }).catch((e) => show(e.message));

// What this plugin was not given: each of these says so.
platypus.request("clipboard", { text: "x" }).catch((e) => {
  document.body.dataset.clipboard = e.message;
});
platypus.request("http", { url: "https://example.com" }).catch((e) => {
  document.body.dataset.http = e.message;
});
