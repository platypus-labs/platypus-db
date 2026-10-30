// Unit tests of the routine debugger's arguments and breakpoints (src/debugger/model.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  breakpointLines,
  initialArguments,
  inputParameters,
  isStopped,
  isTrigger,
  misplacedDefault,
  nextState,
  parameterName,
  withBreakpoint,
} from "../src/debugger/model.ts";

const param = (name, mode = "in", has_default = false) => ({ name, type_name: "integer", mode, has_default });

const routine = (parameters, extra = {}) => ({
  id: "16384",
  schema: "public",
  name: "f",
  signature: "",
  language: "plpgsql",
  procedure: false,
  returns: "integer",
  parameters,
  refusal: null,
  ...extra,
});

test("only IN, INOUT and VARIADIC parameters take a value", () => {
  const r = routine([param("a"), param("r", "out"), param("b", "inout"), param("v", "variadic"), param("c", "table")]);
  assert.deepEqual(
    inputParameters(r).map((p) => p.name),
    ["a", "b", "v"],
  );
});

test("arguments start at their default, or empty", () => {
  const r = routine([param("a"), param("b", "in", true)]);
  assert.deepEqual(initialArguments(r), [{ kind: "value", value: "" }, { kind: "default" }]);
});

test("a default is kept only when every later one is", () => {
  const value = { kind: "value", value: "1" };
  const keep = { kind: "default" };
  assert.equal(misplacedDefault([value, keep, keep]), null);
  assert.equal(misplacedDefault([keep, keep]), null);
  assert.equal(misplacedDefault([]), null);
  assert.equal(misplacedDefault([value, keep, { kind: "null" }]), 1);
  assert.equal(misplacedDefault([keep, value]), 0);
});

test("parameters without a name are named by position", () => {
  assert.equal(parameterName(param(null), 1), "$2");
  assert.equal(parameterName(param("n"), 0), "n");
});

test("breakpoints are kept by routine and line", () => {
  let set = withBreakpoint(new Set(), "1", 4, true);
  set = withBreakpoint(set, "1", 9, true);
  set = withBreakpoint(set, "2", 4, true);
  assert.deepEqual([...breakpointLines(set, "1")].sort(), [4, 9]);
  // `1` is not a prefix of `12`.
  assert.deepEqual([...breakpointLines(withBreakpoint(set, "12", 3, true), "1")].sort(), [4, 9]);
  set = withBreakpoint(set, "1", 4, false);
  assert.deepEqual([...breakpointLines(set, "1")], [9]);
});

test("commands run only on a stopped call; trigger functions are told apart", () => {
  assert.equal(isStopped({ state: "stopped", position: { routine: "1", name: "f()", line: 2 } }), true);
  assert.equal(isStopped({ state: "running" }), false);
  assert.equal(isStopped(null), false);
  assert.equal(isTrigger(routine([], { returns: "trigger" })), true);
  assert.equal(isTrigger(routine([])), false);
});

test("an ended call stays ended, and a stop's abort wins", () => {
  const stopped = { state: "stopped", position: { routine: "1", name: "f()", line: 2 } };
  const outcome = (aborted) => ({ columns: [], rows: [], truncated: false, error: aborted ? "canceled" : null, messages: [], aborted });
  const done = { state: "finished", outcome: outcome(false) };
  const aborted = { state: "finished", outcome: outcome(true) };
  assert.equal(nextState(stopped, { state: "running" }).state, "running");
  assert.equal(nextState({ state: "running" }, stopped), stopped);
  assert.equal(nextState(done, { state: "running" }), done);
  assert.equal(nextState(done, aborted), aborted);
  assert.equal(nextState(aborted, done), aborted);
});
