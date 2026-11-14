// Runs before the app's scripts: paints the loading screen in the colors of the last theme,
// which theme.ts caches under this key ("platypus.appearance.v3"). A file, not an inline
// script, because the content security policy only allows the app's own scripts.
try {
  const cached = JSON.parse(localStorage.getItem("platypus.appearance.v3") ?? "null");
  if (cached) {
    const root = document.documentElement;
    for (const [k, v] of Object.entries(cached.vars)) root.style.setProperty(k, v);
    root.style.colorScheme = cached.dark ? "dark" : "light";
  }
} catch {
  // No cache yet: the system's light or dark colors from index.html.
}
