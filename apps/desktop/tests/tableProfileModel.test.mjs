// Unit tests of the table profile tab's model (src/analysis/tableProfileModel.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  DEFAULT_SAMPLE,
  equalsCondition,
  matchingColumns,
  outlierCondition,
  restoreSample,
} from "../src/analysis/tableProfileModel.ts";

const column = (name, type) => ({ name, db_type: type, data_type: "Utf8" });

test("a saved sample size is one of the offered ones", () => {
  assert.equal(restoreSample(10_000), 10_000);
  assert.equal(restoreSample(null), null);
  assert.equal(restoreSample(12), DEFAULT_SAMPLE);
  assert.equal(restoreSample("x"), DEFAULT_SAMPLE);
  assert.equal(restoreSample(undefined), DEFAULT_SAMPLE);
});

test("the filter box matches names and types, ignoring case", () => {
  const all = [column("Email", "TEXT"), column("age", "INTEGER"), column("created", "TEXT")];
  assert.equal(matchingColumns(all, "").length, 3);
  assert.equal(matchingColumns(all, "  ").length, 3);
  assert.deepEqual(matchingColumns(all, "MAIL").map((c) => c.name), ["Email"]);
  assert.deepEqual(matchingColumns(all, "text").map((c) => c.name), ["Email", "created"]);
  assert.deepEqual(matchingColumns(all, "zzz"), []);
});

test("a value becomes an equality condition with its quotes doubled", () => {
  assert.equal(equalsCondition("sqlite", "email", "o'neil@x.com"), "email = 'o''neil@x.com'");
  assert.equal(equalsCondition("sqlite", `"select"`, ""), `"select" = ''`);
  assert.equal(equalsCondition("postgres", `"a b"`, "x"), `"a b" = 'x'`);
});

test("a value whose literal is not safe is not turned into a condition", () => {
  assert.equal(equalsCondition("mysql", "c", "a\\b"), null);
  assert.equal(equalsCondition("sqlite", "c", "a\\b"), "c = 'a\\b'");
  assert.equal(equalsCondition("sqlite", "c", "a\0b"), null);
});

test("the outliers are the rows past either fence", () => {
  assert.equal(outlierCondition("n", -3.5, 120), "(n < -3.5 OR n > 120)");
});
