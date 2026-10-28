import { registerAction } from "../actions";
import { ExplainTab, openPlan } from "../explain/ExplainTab";
import { registerTabKind } from "./registry";
import { t } from "../i18n";

registerTabKind({
  kind: "explain",
  component: ExplainTab,
  create: (init) => ({ sql: "", analyze: false, ...init }),
  label: (tab) => t(tab.plan ? "Query plan" : tab.analyze ? "Explain analyze" : "Explain"),
  // Not restored: reopening would run an analyzed statement again unasked.
  persist: () => null,
  restore: () => ({}),
});

registerAction({
  id: "editor.explain",
  scope: "editor",
  title: "Explain plan",
  shortcut: "Mod+Shift+E",
  group: "run",
  order: 6,
  when: ({ host }) => !!host?.explain,
  run: ({ host }) => host?.explain?.(false),
});

registerAction({
  id: "editor.explain_analyze",
  scope: "editor",
  title: "Explain analyze (runs the statement)",
  group: "run",
  order: 7,
  when: ({ host }) => !!host?.explain,
  run: ({ host }) => host?.explain?.(true),
});

registerAction({
  id: "explain.open_plan",
  scope: "global",
  title: "Open query plan…",
  run: openPlan,
});

registerAction({
  id: "navigator.open_plan",
  scope: "navigator.list",
  title: "Open query plan…",
  group: "open",
  order: 40,
  run: openPlan,
});
