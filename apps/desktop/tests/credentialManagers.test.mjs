// Unit tests of the password managers' commands (src/credentialManagers.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { MANAGERS, managerOf } from "../src/credentialManagers.ts";

const byId = Object.fromEntries(MANAGERS.map((m) => [m.id, m]));

test("each manager's command reads the secret by its reference", () => {
  assert.equal(byId["1password"].command("op://Work/prod db/password"), 'op read "op://Work/prod db/password"');
  assert.equal(byId.bitwarden.command("prod-db"), 'bw get password "prod-db"');
  assert.equal(byId.vault.command("secret/prod-db"), 'vault kv get -field="password" "secret/prod-db"');
  assert.equal(byId.vault.command("secret/prod-db#pass"), 'vault kv get -field="pass" "secret/prod-db"');
  assert.equal(
    byId.aws.command("prod/db"),
    'aws secretsmanager get-secret-value --secret-id "prod/db" --query SecretString --output text',
  );
});

test("a command made here is recognized again, with its reference", () => {
  for (const m of MANAGERS) {
    for (const reference of [m.example, "with space", ""]) {
      const found = managerOf(m.command(reference));
      assert.equal(found?.manager.id, m.id, m.id);
      assert.equal(found?.reference, reference, m.id);
    }
  }
  assert.equal(managerOf(byId.vault.command("kv/x#token"))?.reference, "kv/x#token");
});

test("other commands are not a manager's", () => {
  assert.equal(managerOf(null), null);
  assert.equal(managerOf(undefined), null);
  assert.equal(managerOf("echo secret"), null);
  assert.equal(managerOf('op read "op://a/b/c" | tr -d x'), null);
  assert.equal(managerOf("op read op://a/b/c"), null, "unquoted: written by hand");
});

test("what the shells would change never reaches a command", () => {
  for (const bad of ['a"b', "a$b", "a`b", "a%PATH%"]) {
    assert.equal(managerOf(byId.bitwarden.command(bad))?.reference, bad.replace(/["`$%]/g, ""), bad);
  }
  // A field or path written to run another command stays one quoted argument.
  assert.equal(byId.vault.command('x#password" ; rm -rf ~ ; "'), 'vault kv get -field="password ; rm -rf ~ ; " "x"');
  assert.equal(byId["1password"].command('a"; echo $HOME'), 'op read "a; echo HOME"');
});
