import { useState } from "react";
import { api, toCommandError, type GitPullOutcome, type GitStatus } from "../api";
import { t } from "../i18n";
import { ask, choose } from "../ui/ask";
import { Icon } from "../ui/Icon";
import { askIdentity, openCommit } from "./GitDialogs";
import { conflicts } from "./gitModel";

interface Props {
  project: string | null;
  /** `null` until the first answer. */
  status: GitStatus | null;
  /** The kind of error that kept the status from being read: `git_not_installed`, or other. */
  problem: { kind: string; message: string } | null;
  /** Reads the status and the scripts again. */
  reload: () => void;
  onError: (message: string) => void;
}

/** The Git bar of the Scripts panel: the branch, how far it is from its upstream, and the
 *  buttons that refresh, pull, push and commit. */
export function GitBar({ project, status, problem, reload, onError }: Props) {
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  /** Runs `job` with the bar locked, then reads everything again; its error goes to the panel. */
  async function run(job: () => Promise<string | null | void>) {
    setBusy(true);
    setNote(null);
    try {
      const done = await job();
      if (done) setNote(done);
    } catch (e) {
      onError(toCommandError(e).message);
    } finally {
      setBusy(false);
      reload();
    }
  }

  /** Runs a call that needs an author: asks for one once if Git has none. */
  async function withAuthor<T>(call: (identity: Awaited<ReturnType<typeof askIdentity>>) => Promise<T>): Promise<T | null> {
    try {
      return await call(null);
    } catch (e) {
      if (toCommandError(e).kind !== "git_identity_missing") throw e;
      const identity = await askIdentity();
      return identity ? call(identity) : null;
    }
  }

  const describe = (o: GitPullOutcome): string =>
    o.kind === "up_to_date"
      ? t("Already up to date")
      : o.kind === "conflicts"
        ? t("The merge stopped on conflicts in: {files}. Resolve them in the files and commit, or abort the merge.", { files: o.files.join(", ") })
        : o.kind === "merged"
          ? t("Merged the remote changes")
          : t("Pulled the latest changes");

  async function pull() {
    let outcome = await api.gitPull(project);
    if (outcome.kind === "diverged") {
      const merge = await choose(
        t("Your scripts and the remote both have new commits, so Git cannot just fast-forward. Merge the remote changes into yours?"),
        [{ label: t("Pull with merge"), value: true, primary: true }],
      );
      if (!merge) return t("Not pulled: the branches have diverged");
      const merged = await withAuthor((identity) => api.gitPullMerge(project, identity));
      if (!merged) return null;
      outcome = merged;
    }
    return describe(outcome);
  }

  async function init() {
    const yes = await ask(t("Turn this project's scripts into a Git repository? A .git folder is created in the scripts folder; no file is changed."), {
      confirm: t("Turn into a repository"),
    });
    if (yes) await api.gitInit(project);
  }

  if (problem?.kind === "git_not_installed") {
    return <div className="git-bar muted small">{t("Git is not installed. Install it to version these scripts.")}</div>;
  }
  if (problem) return <div className="git-bar error-text small">{problem.message}</div>;
  if (!status) return null;
  if (!status.is_repo) {
    return (
      <div className="git-bar">
        <button type="button" className="small" disabled={busy} onClick={() => void run(init)}>
          {t("Turn this project's scripts into a Git repository")}
        </button>
        {note && <div className="muted small">{note}</div>}
      </div>
    );
  }

  const { branch, files, merging } = status;
  const conflicted = conflicts(files);
  return (
    <div className="git-bar" aria-busy={busy}>
      <div className="git-bar-row">
        <span className="git-branch" title={branch.upstream ? t("Tracks {name}", { name: branch.upstream }) : t("No upstream branch")}>
          {branch.name ?? t("detached")}
        </span>
        {branch.upstream && (
          <span className="git-sync muted small" title={t("{ahead} to push, {behind} to pull", { ahead: branch.ahead, behind: branch.behind })}>
            {`↑${branch.ahead} ↓${branch.behind}`}
          </span>
        )}
        <span className="spacer" />
        <button className="icon" disabled={busy} title={t("Refresh: fetch from the remote")} aria-label={t("Refresh")} onClick={() => void run(async () => (branch.upstream ? api.gitFetch(project) : undefined))}>
          <Icon name="refresh" size={13} />
        </button>
        <button type="button" className="small" disabled={busy || merging || !branch.upstream} title={branch.upstream ? undefined : t("No upstream branch")} onClick={() => void run(pull)}>
          {t("Pull")}
        </button>
        <button type="button" className="small" disabled={busy || merging || !branch.has_commits} onClick={() => void run(async () => (await api.gitPush(project), t("Pushed")))}>
          {t("Push")}
        </button>
        <button
          type="button"
          className="small"
          disabled={busy || files.length === 0}
          onClick={() => openCommit(project, files, (id) => (setNote(t("Committed {id}", { id })), reload()))}
        >
          {t("Commit…")}
        </button>
      </div>
      {merging && (
        <div className="notice warn small git-merging">
          <span>{conflicted.length > 0 ? t("Merge in progress. Conflicts: {files}", { files: conflicted.join(", ") }) : t("Merge in progress: commit to conclude it.")}</span>
          <button type="button" className="small" disabled={busy} onClick={() => void run(async () => (await api.gitMergeAbort(project), t("Merge aborted")))}>
            {t("Abort merge")}
          </button>
        </div>
      )}
      {note && <div className="git-note muted small">{note}</div>}
    </div>
  );
}
