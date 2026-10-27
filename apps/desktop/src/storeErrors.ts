/**
 * Messages the connection store (`platypus-store`) gives in English, for the checks of what
 * the user typed: shown in the interface's language. Others pass as they are.
 */
import { msg, t } from "./i18n";

const FIXED = new Set([
  msg("project name is required"),
  msg("connection name is required"),
  msg("profile name is required"),
  msg("initials are one or two characters"),
  msg("a client certificate needs both the certificate and its key"),
  msg("a connection cannot go before itself"),
]);

/** Messages with a value in them: the pattern finds it, the text puts it back. */
const PATTERNS: [RegExp, string][] = [[/^a project named "(.*)" exists already$/s, msg('a project named "{name}" exists already')]];

export function storeMessage(message: string): string {
  if (FIXED.has(message)) return t(message);
  for (const [pattern, text] of PATTERNS) {
    const m = pattern.exec(message);
    if (m) return t(text, { name: m[1]! });
  }
  return message;
}
