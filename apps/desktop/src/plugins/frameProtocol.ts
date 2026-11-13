// What a plugin's frame may say to the app, read before anything acts on it. The frame's HTML is
// untrusted: a message is a small object of known fields, whose request is one of a short list and
// whose parameters are one bounded JSON text. Kept apart from the frame's host, without the app
// around it. The backend checks all of it again.
import type { PluginPermissions } from "../api";

/** The longest parameters of a request, as JSON text. */
export const MAX_PARAMS = 1 << 20;
/** Requests waiting for their answer at one time, per frame. */
export const MAX_IN_FLIGHT = 16;
/** Requests per second a frame may make, and the burst it may start with. */
const RATE_PER_SECOND = 50;
const RATE_BURST = 100;
/** Messages dropped over the rate before the frame is stopped. */
const MAX_DROPPED = 500;
/** Messages that are not what they should be before the frame is stopped. */
const MAX_BROKEN = 3;
/** How long a frame waits between requests that the user sees: a message, a new tab. */
const NOISY_MS = 1000;

/** All a frame can ask. */
export const FRAME_REQUESTS = ["setting", "notify", "clipboard", "open_query", "http", "run_action", "state_get", "state_set"] as const;

export type FrameRequestName = (typeof FRAME_REQUESTS)[number];

/** A message of a frame, as far as it can be read.
 *  `broken`: not one of the app's messages at all. `refused`: a request to say no to, and with
 *  `strike` one no page of good faith sends: asking for a request this version does not have is
 *  not that, but parameters past the limit are. */
type FrameMessage =
  | { kind: "pong"; n: number }
  | { kind: "request"; id: number; name: FrameRequestName; params: unknown }
  | { kind: "refused"; id: number; reason: string; strike: boolean }
  | { kind: "broken" };

const BROKEN: FrameMessage = { kind: "broken" };

/** Reads `data`, a message's payload. Only fields of known types count, and one that has others
 *  (a way to send something huge past the bound on `params`) is broken. */
export function readFrameMessage(data: unknown): FrameMessage {
  if (typeof data !== "object" || data === null || Array.isArray(data)) return BROKEN;
  const m = data as Record<string, unknown>;
  if (m.platypus !== 1) return BROKEN;
  const fields = Object.keys(m).length;
  // A count: `NaN` would make the watchdog's sum `NaN`, and a frame that never counts as hung.
  if (m.type === "pong") return fields === 3 && Number.isSafeInteger(m.n) ? { kind: "pong", n: m.n as number } : BROKEN;
  if (m.type !== "request" || fields !== 5 || !Number.isSafeInteger(m.id)) return BROKEN;
  const id = m.id as number;
  if (typeof m.name !== "string" || typeof m.params !== "string") return BROKEN;
  if (!(FRAME_REQUESTS as readonly string[]).includes(m.name)) return { kind: "refused", id, reason: `no such request: ${m.name.slice(0, 64)}`, strike: false };
  if (m.params.length > MAX_PARAMS) return { kind: "refused", id, reason: `the request is larger than ${MAX_PARAMS >> 10} KB`, strike: true };
  try {
    return { kind: "request", id, name: m.name as FrameRequestName, params: JSON.parse(m.params) };
  } catch {
    return { kind: "refused", id, reason: "the parameters are not JSON", strike: true };
  }
}

/** Why the plugin may not make this request, by what it was approved; the backend asks again. */
export function refusedBy(name: FrameRequestName, approved: PluginPermissions): string | null {
  if (name === "clipboard" && !approved.clipboard) return "not allowed: this plugin has no permission for the clipboard";
  if (name === "http" && approved.http.length === 0) return "not allowed: this plugin has no permission for HTTP";
  return null;
}

/** What to do with a message that came in. */
type Admission = "serve" | "drop" | "stop";

/** Holds a frame to its rate and stops one that keeps breaking the rules. */
export class FrameGuard {
  private tokens = RATE_BURST;
  private at: number;
  private dropped = 0;
  private broken = 0;
  private lastNoisy = Number.NEGATIVE_INFINITY;

  constructor(now: number) {
    this.at = now;
  }

  /** One more message, at `now` milliseconds. */
  admit(now: number): Admission {
    this.tokens = Math.min(RATE_BURST, this.tokens + ((now - this.at) / 1000) * RATE_PER_SECOND);
    this.at = now;
    if (this.tokens >= 1) {
      this.tokens -= 1;
      return "serve";
    }
    this.dropped += 1;
    return this.dropped > MAX_DROPPED ? "stop" : "drop";
  }

  /** A message that is not one of the app's; `true` when that was too many. */
  strike(): boolean {
    this.broken += 1;
    return this.broken >= MAX_BROKEN;
  }

  /** May a request the user sees go ahead at `now`? */
  noisy(now: number): boolean {
    if (now - this.lastNoisy < NOISY_MS) return false;
    this.lastNoisy = now;
    return true;
  }
}

/** How many pings a frame may leave unanswered before it counts as hung. */
const MAX_MISSED_PINGS = 3;

/** Whether a frame that has answered up to `answered`, having been pinged up to `sent`, is hung. */
export const hung = (sent: number, answered: number) => sent - answered > MAX_MISSED_PINGS;
