// The menu of a dashboard widget: what its tile offers under the "⋯" button.
import { registerAction } from "../actions";
import { msg } from "../i18n";
import type { Widget } from "./model";
import type { WidgetRun } from "./run";

declare module "../actions" {
  interface ActionScopes {
    /** A widget of a dashboard tab, with what its tile does for each action. */
    "dashboard.widget": {
      widget: Widget;
      run: WidgetRun;
      first: boolean;
      last: boolean;
      configuring: boolean;
      onRefresh: () => void;
      onConfigure: () => void;
      onEdit: () => void;
      onMove: (by: -1 | 1) => void;
      onRemove: () => void;
    };
  }
}

registerAction({
  id: "dashboard.chart_settings",
  scope: "dashboard.widget",
  title: "Chart settings",
  group: "view",
  order: 1,
  when: ({ widget }) => widget.view === "chart",
  run: ({ onConfigure }) => onConfigure(),
});

registerAction({
  id: "dashboard.refresh",
  scope: "dashboard.widget",
  title: "Refresh",
  group: "view",
  order: 2,
  disabled: ({ run }) => (run.running ? msg("Executing…") : null),
  run: ({ onRefresh }) => onRefresh(),
});

registerAction({
  id: "dashboard.edit",
  scope: "dashboard.widget",
  title: "Edit",
  group: "edit",
  order: 10,
  run: ({ onEdit }) => onEdit(),
});

registerAction({
  id: "dashboard.up",
  scope: "dashboard.widget",
  title: "Move up",
  group: "edit",
  order: 11,
  disabled: ({ first }) => (first ? msg("Already first") : null),
  run: ({ onMove }) => onMove(-1),
});

registerAction({
  id: "dashboard.down",
  scope: "dashboard.widget",
  title: "Move down",
  group: "edit",
  order: 12,
  disabled: ({ last }) => (last ? msg("Already last") : null),
  run: ({ onMove }) => onMove(1),
});

registerAction({
  id: "dashboard.remove",
  scope: "dashboard.widget",
  title: "Remove",
  group: "remove",
  order: 20,
  run: ({ onRemove }) => onRemove(),
});
