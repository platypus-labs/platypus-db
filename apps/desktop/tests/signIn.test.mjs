// Unit tests of the ways to sign in without a password (src/signIn.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { signInOf, signInsFor, SIGN_INS, withSignIn } from "../src/signIn.ts";
import { testable } from "../src/connectionTest.ts";

const base = { id: "1", name: "c", driver: "postgres", host: "db.rds.amazonaws.com", port: null, database: "d", user: "app", save_password: false, options: {} };

test("a connection signs in the way its auth option names", () => {
  assert.equal(signInOf(base), undefined);
  assert.equal(signInOf({ options: { auth: "password" } }), undefined);
  assert.equal(signInOf({ options: { auth: "entra_id" } })?.name, "Microsoft Entra ID");
});

test("each database offers its own ways, and keeps the one it has", () => {
  assert.deepEqual(signInsFor("postgres").map((s) => s.id), ["aws_iam", "gcp_iam", "entra_id", "kerberos"]);
  for (const driver of ["mysql", "sqlserver", "sqlite", "mongo"]) assert.ok(!signInsFor(driver).some((s) => s.id === "kerberos"), driver);
  assert.deepEqual(signInsFor("sqlserver").map((s) => s.id), ["entra_id", "windows"]);
  assert.deepEqual(signInsFor("sqlserver", "aws_iam").map((s) => s.id), ["aws_iam", "entra_id", "windows"]);
  assert.deepEqual(signInsFor("sqlite"), []);
});

test("choosing a way sets or removes the option, and leaves the others", () => {
  const signed = withSignIn({ ...base, options: { pooler: "transaction" } }, "aws_iam");
  assert.deepEqual(signed.options, { pooler: "transaction", auth: "aws_iam" });
  assert.deepEqual(withSignIn(signed, null).options, { pooler: "transaction" });
  // The Kerberos service name goes with the way it belongs to.
  const kerberos = withSignIn({ ...base, options: { krb_service: "pg" } }, "kerberos");
  assert.deepEqual(kerberos.options, { krb_service: "pg", auth: "kerberos" });
  assert.deepEqual(withSignIn(kerberos, "aws_iam").options, { auth: "aws_iam" });
  assert.deepEqual(withSignIn(kerberos, null).options, {});
});

test("the AWS command names the host, port, user, region and profile", () => {
  const aws = SIGN_INS.find((s) => s.id === "aws_iam");
  assert.equal(
    aws.command({ ...base, options: { aws_region: "eu-west-1" } }),
    "aws rds generate-db-auth-token --hostname db.rds.amazonaws.com --port 5432 --username app --region eu-west-1",
  );
  assert.match(aws.command({ ...base, driver: "mysql", options: { aws_profile: "prod" } }), /--port 3306 .*--profile prod$/);
  assert.equal(SIGN_INS.find((s) => s.id === "windows").command, null);
});

test("connections that sign in without a password are tested", () => {
  assert.equal(testable(base), false);
  assert.equal(testable({ ...base, options: { auth: "gcp_iam" } }), true);
  assert.equal(testable({ ...base, options: { auth: "kerberos" } }), true);
  assert.equal(testable({ ...base, options: { auth: "password" } }), false);
  assert.equal(testable({ ...base, options: { auth: "ntlm" } }), false);
  for (const s of SIGN_INS) assert.equal(testable({ ...base, options: { auth: s.id } }), true, s.id);
});
