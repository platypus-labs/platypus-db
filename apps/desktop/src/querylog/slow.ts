// The threshold past which a query counts as slow: the editor's status bar and the query
// manager mark it.
import { defineSetting } from "../settings";

export const slowQuerySeconds = defineSetting<number>({
  key: "performance.slow_query_seconds",
  section: "Performance",
  label: "Mark queries slower than (seconds)",
  description: "0 marks none. The status bar and the query manager show them.",
  default: 5,
  control: { type: "number", min: 0, max: 86400, step: 0.5 },
});

/** Whether `ms` passes the threshold `seconds` (0: never). */
export const isSlow = (ms: number | null, seconds: number) => seconds > 0 && ms !== null && ms >= seconds * 1000;
