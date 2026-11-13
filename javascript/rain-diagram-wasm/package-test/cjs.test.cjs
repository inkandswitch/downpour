// The built package through `require`, as CommonJS code loads it.
const assert = require("node:assert/strict");
const { test } = require("node:test");
const { renderText, RainDiagram } = require("@inkandswitch/rain-diagram-wasm");

test("require: ready to use, no init", () => {
  assert.match(renderText("rain\n(A)-[read(A)]->(B)"), /\(A\)\s+\(B\)/);
  const d = new RainDiagram("rain\n(A)-[read(A)]->(B)");
  try {
    assert.equal(d.kind, "rain");
  } finally {
    d.free();
  }
});
