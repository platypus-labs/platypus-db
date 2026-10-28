/**
 * Messages the app's backend gives in English, shown in the interface's language: the checks
 * of the connection store (`platypus-store`) on what the user typed, and PlatypusDB's own
 * errors (a file not found, a read-only connection, a connection URL that does not read). The
 * databases' messages pass as they are.
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

/** The errors of a connection URL or string that does not read (`platypus-core`'s config). */
const URL_ERRORS = new Set([
  msg("not a connection URL (postgres://…, jdbc:…) or connection string (Server=…;Database=…)"),
  msg("the MongoDB URL has no host"),
  msg("the host is not valid: it holds `@`, `=`, `;`, spaces or a `:` that is not part of an IPv6 address"),
  msg("the host name of the URL is not valid"),
  msg("the IPv6 zone of the URL is empty"),
  msg("a JDBC URL starts with `jdbc:`"),
  msg("the user and password of the JDBC URL must be percent-encoded"),
  msg("the connection string has no key=value pairs"),
  msg("a part of the connection string has no `=`"),
  msg("named pipes and shared memory are not supported: the server needs a TCP address"),
]);

/** The reasons the `url` crate gives for a URL it refuses, after "invalid URL: ". */
const URL_REASONS = new Set([
  msg("empty host"),
  msg("invalid international domain name"),
  msg("invalid port number"),
  msg("invalid IPv4 address"),
  msg("invalid IPv6 address"),
  msg("invalid domain character"),
  msg("relative URL without a base"),
]);

const reason = (detail: string) => (URL_REASONS.has(detail) ? t(detail) : detail);

const URL_PATTERNS: [RegExp, string, string[]][] = [
  [/^unsupported URL scheme `(.*)`$/s, msg("unsupported URL scheme `{scheme}`"), ["scheme"]],
  [/^unknown sslmode `(.*)`$/s, msg("unknown sslmode `{value}`"), ["value"]],
  [/^unknown URL parameter `(.*)`$/s, msg("unknown URL parameter `{key}`"), ["key"]],
  [/^invalid connect_timeout `(.*)`$/s, msg("invalid connect_timeout `{value}`"), ["value"]],
  [/^invalid connectTimeoutMS `(.*)`$/s, msg("invalid connectTimeoutMS `{value}`"), ["value"]],
  [/^invalid tls `(.*)`$/s, msg("invalid tls `{value}`"), ["value"]],
  [/^invalid percent-encoding: (.*)$/s, msg("invalid percent-encoding: {detail}"), ["detail"]],
  [/^unsupported JDBC URL `(.*)`$/s, msg("unsupported JDBC URL `{url}`"), ["url"]],
  [/^unsupported provider `(.*)`$/s, msg("unsupported provider `{provider}`"), ["provider"]],
  [/^unexpected text after the quoted value of `(.*)`$/s, msg("unexpected text after the quoted value of `{key}`"), ["key"]],
  [/^the quoted value of `(.*)` is not closed$/s, msg("the quoted value of `{key}` is not closed"), ["key"]],
  [/^`(.*)` is not a valid number$/s, msg("`{key}` is not a valid number"), ["key"]],
  [
    /^the file of a (\S+) URL goes after three slashes, as in \S+:\/\/\/C:\/data\/app\.db$/s,
    msg("the file of a {scheme} URL goes after three slashes, as in {scheme}:///C:/data/app.db"),
    ["scheme"],
  ],
];

/** A connection URL's error in the interface's language, or `null` when it is not one. */
function urlMessage(message: string): string | null {
  const invalid = /^invalid URL: (.*)$/s.exec(message);
  if (invalid) return t("invalid URL: {reason}", { reason: reason(invalid[1] ?? "") });
  return translate(message, URL_ERRORS, URL_PATTERNS);
}

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
  return translate(message, APP, APP_PATTERNS) ?? urlMessage(message) ?? storeMessage(message);
}
