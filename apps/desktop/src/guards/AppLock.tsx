/** The app's lock: after some minutes without the user (or at once, from the palette), the
 *  window hides behind the master password, which locks again, and the production sessions
 *  are closed. It needs a master password: without one there is nothing to lock with. */
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, toCommandError } from "../api";
import { defineSetting, useSetting } from "../settings";
import mark from "../assets/brand/platypus-mark.png";
import { t } from "../i18n";

export const lockSetting = defineSetting<number>({
  key: "safety.lock_minutes",
  section: "Safety",
  label: "Lock the app after this many idle minutes",
  description: "0 never locks. It needs a master password; production connections are closed when it locks.",
  default: 0,
  control: { type: "number", min: 0, max: 1440, step: 1 },
});

/** The user's activity that keeps the app open. */
const ACTIVITY = ["keydown", "mousedown", "mousemove", "wheel", "touchstart"] as const;

/** Calls `lock` once the user has been away for the setting's minutes; not while `locked`. */
export function useIdleLock(lock: () => void, locked: boolean) {
  const minutes = useSetting(lockSetting);
  const last = useRef(Date.now());
  const latest = useRef(lock);
  latest.current = lock;
  useEffect(() => {
    if (minutes <= 0 || locked) return;
    last.current = Date.now();
    const seen = () => {
      last.current = Date.now();
    };
    for (const name of ACTIVITY) window.addEventListener(name, seen, { capture: true, passive: true });
    // The user at another window of the app is not away either.
    const elsewhere = listen("platypus://activity", seen);
    const timer = setInterval(() => {
      if (Date.now() - last.current >= minutes * 60_000) latest.current();
    }, 15_000);
    return () => {
      clearInterval(timer);
      for (const name of ACTIVITY) window.removeEventListener(name, seen, { capture: true });
      void elsewhere.then((stop) => stop());
    };
  }, [minutes, locked]);
}

/** Covers the window until the master password is entered. */
export function LockScreen({ onUnlocked }: { onUnlocked: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const backdrop = useRef<HTMLDivElement>(null);

  // Everything else in the window is inert while it is locked: no focus, click or key
  // reaches it, the editor's own shortcuts included.
  useEffect(() => {
    const parent = backdrop.current?.parentElement;
    if (!parent) return;
    const others = [...parent.children].filter((el): el is HTMLElement => el !== backdrop.current && el instanceof HTMLElement && !el.inert);
    for (const el of others) el.inert = true;
    return () => {
      for (const el of others) el.inert = false;
    };
  }, []);

  async function unlock(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.unlockVault(password);
      onUnlocked();
    } catch (err) {
      setError(toCommandError(err).message);
      setPassword("");
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop lock-screen" ref={backdrop}>
      <form className="modal small lock-dialog" onSubmit={unlock} onKeyDown={(e) => e.key === "Escape" && e.preventDefault()}>
        <img className="lock-mark" src={mark} alt="" />
        <h2>{t("PlatypusDB is locked")}</h2>
        <p className="muted small">{t("Enter the master password to go on. Production connections were closed: they reconnect when used.")}</p>
        <label className="field">
          <span>{t("Master password")}</span>
          <input autoFocus type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        </label>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="submit" className="primary" disabled={busy || password === ""}>
            {t("Unlock")}
          </button>
        </div>
      </form>
    </div>
  );
}
