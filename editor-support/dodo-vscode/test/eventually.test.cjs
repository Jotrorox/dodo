const assert = require("node:assert/strict");
const { test } = require("node:test");
const { eventually } = require("./eventually.cjs");

test("readiness waits for a value", async () => {
  let calls = 0;
  const value = await eventually("provider", () => ++calls > 1 && "ready", { intervalMs: 1 });
  assert.equal(value, "ready");
  assert.equal(calls, 2);
});

test("readiness retries VS Code cancellations", async () => {
  for (const name of ["Canceled", "CancellationError"]) {
    let calls = 0;
    const value = await eventually("editor", () => {
      if (++calls === 1) throw Object.assign(new Error("Canceled"), { name });
      return "ready";
    }, { intervalMs: 1 });
    assert.equal(value, "ready");
    assert.equal(calls, 2);
  }
});

test("readiness propagates other errors immediately", async () => {
  const failure = new Error("provider failed");
  let calls = 0;
  await assert.rejects(eventually("provider", () => {
    calls++;
    throw failure;
  }), (error) => error === failure);
  assert.equal(calls, 1);
});

test("persistent cancellations fail with the readiness label and cause", async () => {
  const cancellation = Object.assign(new Error("Canceled"), { name: "Canceled" });
  await assert.rejects(eventually("editor", () => {
    throw cancellation;
  }, { timeoutMs: 25, intervalMs: 1 }), (error) => {
    assert.equal(error.message, "Timed out waiting for editor");
    assert.equal(error.cause, cancellation);
    return true;
  });
});
