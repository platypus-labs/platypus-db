/**
 * Messages the app's backend gives in English, shown in the interface's language: the checks
 * of the connection store (`platypus-store`) on what the user typed, and PlatypusDB's own
 * errors (a file not found, a read-only connection). The databases' messages pass as they are.
 */
import { msg, t } from "./i18n";

const STORE = new Set([
  msg("project name is required"),
  msg("connection name is required"),
  msg("profile name is required"),
  msg("initials are one or two characters"),
  msg("a client certificate needs both the certificate and its key"),
  msg("a connection cannot go before itself"),
]);

/** Messages with values in them: the pattern finds them, in the order of the text's names. */
const STORE_PATTERNS: [RegExp, string, string[]][] = [[/^a project named "(.*)" exists already$/s, msg('a project named "{name}" exists already'), ["name"]]];

const APP = new Set([
  msg("Query cancelled"),
  msg("Connection closed"),
  msg("no database file given"),
  msg("commit or roll back the open transaction first"),
  msg("The connection was lost and has been reopened; the open transaction was rolled back."),
  msg(
    "The statement did not stop on Cancel, so its connection was closed and a new one is opened; the open transaction was rolled back.",
  ),
  msg("the table must be rebuilt outside a transaction: commit or roll back first"),
]);

const APP_PATTERNS: [RegExp, string, string[]][] = [
  [/^(.*) does not exist, nor does its folder$/s, msg("{path} does not exist, nor does its folder"), ["path"]],
  [
    /^(.*) not found: a relative path is not created, give the full path of the file$/s,
    msg("{path} not found: a relative path is not created, give the full path of the file"),
    ["path"],
  ],
  [/^(.*) is read-only: nothing that may change it runs there$/s, msg("{name} is read-only: nothing that may change it runs there"), ["name"]],
  [
    /^the download failed \(check the internet connection\): (.*)$/s,
    msg("the download failed (check the internet connection): {detail}"),
    ["detail"],
  ],
  [/^cannot reach the driver registry: (.*)$/s, msg("cannot reach the driver registry: {detail}"), ["detail"]],
];

function translate(message: string, fixed: Set<string>, patterns: [RegExp, string, string[]][]): string | null {
  if (fixed.has(message)) return t(message);
  for (const [pattern, text, names] of patterns) {
    const m = pattern.exec(message);
    if (m) return t(text, Object.fromEntries(names.map((name, i) => [name, m[i + 1] ?? ""])));
  }
  return null;
}

export function storeMessage(message: string): string {
  return translate(message, STORE, STORE_PATTERNS) ?? message;
}

/** A message of the app's backend (not a database's) in the interface's language. */
export function appMessage(message: string): string {
  return translate(message, APP, APP_PATTERNS) ?? storeMessage(message);
}
