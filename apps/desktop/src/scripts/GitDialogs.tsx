// The dialogs of the Scripts panel's Git bar: the author, the changes of a script, a commit.
import { useEffect, useState } from "react";
import { api, toCommandError, type GitFileDiff, type GitFileStatus, type GitIdentity } from "../api";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { selectedPaths, validIdentity } from "./gitModel";
import { nameOf } from "./scripts";

/** The word that stands for a status in the list: the badge's meaning. */
export function badgeTitle(badge: GitFileStatus["badge"]): string {
  switch (badge) {
    case "M":
      return t("Modified");
    case "A":
      return t("Added");
    case "D":
      return t("Deleted");
    case "R":
      return t("Renamed");
    case "U":
      return t("Conflict");
    default:
      return t("Not tracked yet");
  }
}

interface IdentityProps {
  onAnswer: (identity: GitIdentity | null) => void;
}

function IdentityDialog({ onAnswer }: IdentityProps) {
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onAnswer(null)}>
      <form
        className="modal git-identity"
        onKeyDown={(e) => e.key === "Escape" && onAnswer(null)}
        onSubmit={(e) => {
          e.preventDefault();
          if (validIdentity(name, email)) onAnswer({ name: name.trim(), email: email.trim() });
        }}
      >
        <h2>{t("Who are you?")}</h2>
        <p className="muted small">{t("Git does not know the author of commits. This is saved in the scripts repository only, not for your other repositories.")}</p>
        <label className="field">
          <span>{t("Name")}</span>
          <input autoFocus value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field">
          <span>{t("Email")}</span>
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
        </label>
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={() => onAnswer(null)}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" disabled={!validIdentity(name, email)}>
            {t("Save")}
          </button>
        </div>
      </form>
    </div>
  );
}

/** Asks for the author of commits; `null` when the dialog is cancelled. */
export function askIdentity(): Promise<GitIdentity | null> {
  return new Promise((resolve) =>
    workbench().openDialog((close) => (
      <IdentityDialog
        onAnswer={(identity) => {
          close();
          resolve(identity);
        }}
      />
    )),
  );
}

interface DiffProps {
  project: string | null;
  path: string;
  onClose: () => void;
}

function DiffDialog({ project, path, onClose }: DiffProps) {
  const [diff, setDiff] = useState<GitFileDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    api.gitDiff(project, path).then(setDiff, (e) => setError(toCommandError(e).message));
  }, [project, path]);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal wide git-diff" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("Changes in {name}", { name: nameOf(path) })}</h2>
        {error && <div className="notice error small">{error}</div>}
        {diff?.binary && <p className="muted">{t("A binary file: there is no text to compare.")}</p>}
        {diff && !diff.binary && diff.hunks.length === 0 && <p className="muted">{t("No changes against the last commit.")}</p>}
        {diff && diff.hunks.length > 0 && (
          <div className="git-diff-body" role="table" aria-label={t("Changes in {name}", { name: nameOf(path) })}>
            {diff.hunks.map((hunk, i) => (
              <div key={i} role="rowgroup">
                <div className="git-diff-hunk">{hunk.header}</div>
                {hunk.lines.map((line, j) => (
                  <div key={j} role="row" className={`git-diff-line ${line.kind}`}>
                    <span className="git-diff-no">{line.old_no ?? ""}</span>
                    <span className="git-diff-no">{line.new_no ?? ""}</span>
                    <span className="git-diff-sign">{line.kind === "added" ? "+" : line.kind === "removed" ? "-" : ""}</span>
                    <span className="git-diff-text">{line.text}</span>
                  </div>
                ))}
              </div>
            ))}
          </div>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" autoFocus onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Shows what changed in a script against the last commit, read-only. */
export function openDiff(project: string | null, path: string) {
  workbench().openDialog((close) => <DiffDialog project={project} path={path} onClose={close} />);
}

interface CommitProps {
  project: string | null;
  files: GitFileStatus[];
  onClose: () => void;
  /** Called with the short id once the commit is made. */
  onDone: (id: string) => void;
}

function CommitDialog({ project, files, onClose, onDone }: CommitProps) {
  const [unticked, setUnticked] = useState<Set<string>>(new Set());
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const paths = selectedPaths(files, unticked);
  const ready = paths.length > 0 && message.trim() !== "" && !busy;

  async function commit() {
    setBusy(true);
    setError(null);
    try {
      let identity: GitIdentity | null = null;
      for (;;) {
        try {
          const id = await api.gitCommit(project, paths, message, identity);
          onDone(id);
          return onClose();
        } catch (e) {
          // Git has no author yet: ask once, then try again with it.
          if (toCommandError(e).kind !== "git_identity_missing" || identity) throw e;
          identity = await askIdentity();
          if (!identity) return;
        }
      }
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  function toggle(path: string) {
    setUnticked((u) => {
      const next = new Set(u);
      if (!next.delete(path)) next.add(path);
      return next;
    });
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <form
        className="modal git-commit"
        onKeyDown={(e) => e.key === "Escape" && !busy && onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          if (ready) void commit();
        }}
      >
        <h2>{t("Commit changes")}</h2>
        <ul className="git-commit-files">
          {files.map((f) => (
            <li key={f.path}>
              <label>
                <input type="checkbox" checked={!unticked.has(f.path)} disabled={busy} onChange={() => toggle(f.path)} />
                <span className={`git-badge ${f.badge === "?" ? "untracked" : f.badge}`} title={badgeTitle(f.badge)}>
                  {f.badge}
                </span>
                <span className="git-commit-path">{f.path}</span>
              </label>
            </li>
          ))}
        </ul>
        <label className="field">
          <span>{t("Message")}</span>
          <textarea autoFocus rows={3} value={message} disabled={busy} onChange={(e) => setMessage(e.target.value)} />
        </label>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" disabled={busy} onClick={onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary" disabled={!ready}>
            {t("Commit")}
          </button>
        </div>
      </form>
    </div>
  );
}

/** The Commit dialog: the changed `files` with a box each, and a message. */
export function openCommit(project: string | null, files: GitFileStatus[], onDone: (id: string) => void) {
  workbench().openDialog((close) => <CommitDialog project={project} files={files} onClose={close} onDone={onDone} />);
}
