// Unit tests of the confirmation toasts' store (src/ui/toast.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { dismissToast, getToasts, subscribeToasts, toast } from "../src/ui/toast.ts";

test("a toast shows, notifies and goes after its lifetime", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  let calls = 0;
  const off = subscribeToasts(() => calls++);
  toast("Copied");
  assert.deepEqual(getToasts().map((i) => i.message), ["Copied"]);
  t.mock.timers.tick(2000);
  assert.equal(getToasts().length, 0);
  assert.equal(calls, 2);
  off();
});

test("at most three stack, the oldest goes first; dismiss removes one", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  for (const m of ["a", "b", "c", "d"]) toast(m);
  assert.deepEqual(getToasts().map((i) => i.message), ["b", "c", "d"]);
  dismissToast(getToasts()[0].id);
  assert.deepEqual(getToasts().map((i) => i.message), ["c", "d"]);
  t.mock.timers.tick(2000);
  assert.equal(getToasts().length, 0);
});
