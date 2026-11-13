// The built package (`nix run .#bodge`) as a consumer loads it: by name,
// through its `exports` map. Node resolves a package's own name from
// inside it, so no install is needed.
//
//   node --test javascript/rain-diagram-wasm/package-test/*.test.*

import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import { RainDiagram, renderSvg, renderText, validate } from "@inkandswitch/rain-diagram-wasm";
import * as slim from "@inkandswitch/rain-diagram-wasm/slim";

const SOURCE = "rain\nl: (Doc)-[edit]->(Alice)\n(Doc)-<revoke>-x[l]\n";

test("import: ready to use, no init", () => {
  assert.equal(validate(SOURCE), undefined);
  assert.match(renderText(SOURCE), /\(Doc\)\s+\(Alice\)/);
  assert.match(renderSvg(SOURCE), /^<svg/);
  const d = new RainDiagram(SOURCE);
  try {
    assert.equal(d.events, 2);
  } finally {
    d.free();
  }
});

test("errors still say line:column", () => {
  assert.throws(() => validate("rain\n(A)-[write]->(B)"), /^Error: 2:6: unknown access level/);
});

test("./slim needs initSync with the bytes from ./wasm", async () => {
  const wasm = fs.readFileSync(new URL(import.meta.resolve("@inkandswitch/rain-diagram-wasm/wasm")));
  slim.initSync({ module: wasm });
  assert.match(slim.renderText(SOURCE), /\(Doc\)/);
});

test("./iife defines a global for script tags", async () => {
  const code = fs.readFileSync(new URL(import.meta.resolve("@inkandswitch/rain-diagram-wasm/iife")), "utf8");
  const scope = {};
  new Function("globalThis", `${code}\nglobalThis.RainDiagramWasm = RainDiagramWasm;`)(scope);
  assert.match(scope.RainDiagramWasm.renderText(SOURCE), /\(Doc\)/);
});
