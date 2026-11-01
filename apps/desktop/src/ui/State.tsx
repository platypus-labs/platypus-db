/**
 * The empty, loading and error states of every panel, list and page: one shape everywhere,
 * so that the user recognises what is happening and finds the useful action in the same place.
 *
 * A state is either a block (icon, title, a sentence, actions: for a page, a tab or a panel with
 * room) or `compact` (one muted line: for a list or a small panel). The caller's `className`
 * stays on the element, so the existing selectors keep working.
 */
import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon";

interface StateProps {
  /** One short line: what is (or is not) there. */
  title: ReactNode;
  /** A sentence that says why, or what to do. */
  children?: ReactNode;
  /** The useful actions, usually buttons. */
  actions?: ReactNode;
  icon?: IconName;
  compact?: boolean;
  className?: string;
}

function State({
  kind,
  title,
  children,
  actions,
  icon,
  compact,
  className,
  role,
}: StateProps & { kind: "empty" | "loading" | "error"; role?: "status" | "alert" }) {
  const classes = ["state", kind, compact && "compact", className].filter(Boolean).join(" ");
  const glyph = icon ?? kind;
  if (compact) {
    return (
      <div className={classes} role={role} aria-busy={kind === "loading" || undefined}>
        {kind !== "empty" && <Icon name={glyph} className="state-icon" />}
        <span>{title}</span>
        {children && <span className="state-text">{children}</span>}
        {actions}
      </div>
    );
  }
  return (
    <div className={classes} role={role} aria-busy={kind === "loading" || undefined}>
      <span className="state-pic">
        <Icon name={glyph} size={20} className="state-icon" />
      </span>
      <p className="state-title">{title}</p>
      {children && <p className="state-text">{children}</p>}
      {actions && <div className="state-actions">{actions}</div>}
    </div>
  );
}

export function EmptyState(props: StateProps) {
  return <State kind="empty" {...props} />;
}

export function LoadingState(props: StateProps) {
  return <State kind="loading" role="status" {...props} />;
}

export function ErrorState(props: StateProps) {
  return <State kind="error" role="alert" {...props} />;
}

/**
 * The inline form of waiting, for an action under way: a spinner with an optional label. In a
 * button, set `aria-busy` on the button and put `<Busy />` before its "…ing" label.
 */
export function Busy({ label, className }: { label?: ReactNode; className?: string }) {
  return (
    <span className={["busy", className].filter(Boolean).join(" ")} role="status">
      <Icon name="loading" className="busy-icon" />
      {label}
    </span>
  );
}
