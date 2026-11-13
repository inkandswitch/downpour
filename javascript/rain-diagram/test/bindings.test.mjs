// The Wasm bindings as JS sees them: every export, its errors, and the
// number boundary. Needs the Wasm built into ../pkg (`nix run .#ci-js` does
// that; so does the preview).
//
//   node --test javascript/rain-diagram/test/

import assert from "node:assert/strict";
import fs from "node:fs";
import { describe, test } from "node:test";

const pkg = new URL("../pkg/", import.meta.url);
const wasm = await import(new URL("rain_diagram_wasm.js", pkg));
wasm.initSync({ module: fs.readFileSync(new URL("rain_diagram_wasm_bg.wasm", pkg)) });

const SOURCE = "rain\nl: (Doc)-[edit(Doc)]->(Alice)\n(Alice)-[edit(Doc)]->(Bob)\n(Doc)-<revoke>-x[l]\n";

/** Assert `f` throws an `Error` whose message matches `pattern`. */
const throwsMatching = (f, pattern) => assert.throws(f, (e) => e instanceof Error && pattern.test(e.message));

describe("one-shot functions", () => {
  test("validate passes good source and throws line:column for bad", () => {
    assert.equal(wasm.validate(SOURCE), undefined);
    throwsMatching(() => wasm.validate("rain\n(A)-[write]->(B)"), /^2:6: unknown access level "write"/);
  });

  test("diagnostics are line:column strings", () => {
    assert.deepEqual(wasm.diagnostics(SOURCE), []);
    const [warning] = wasm.diagnostics("rain\n(Alice)-[edit(Doc)]->(Bob)");
    assert.match(warning, /^2:1: Alice holds no authority over Doc/);
  });

  test("renders", () => {
    assert.match(wasm.renderSvg(SOURCE), /^<svg [^>]*class="rain-diagram"/);
    assert.match(wasm.renderText(SOURCE), /\(Doc\)\s+\(Alice\)\s+\(Bob\)/);
    const site = JSON.parse(wasm.toSiteJson(SOURCE));
    assert.equal(site.agents.length, 3);
    assert.equal(site.events.length, 3);
    throwsMatching(() => wasm.renderSvg("nope"), /^1:1: unknown diagram kind/);
  });
});

describe("edits", () => {
  test("setPosition writes dashes, and undefined puts it back", () => {
    const placed = wasm.setPosition(SOURCE, 1, 75);
    assert.match(placed, /\(Alice\)---\[edit\(Doc\)\]->\(Bob\)/);
    assert.equal(wasm.setPosition(placed, 1, undefined), SOURCE);
  });

  test("moveStatement moves within range and refuses outside it", () => {
    assert.equal(
      wasm.moveStatement(SOURCE, 1, 2),
      "rain\nl: (Doc)-[edit(Doc)]->(Alice)\n(Doc)-<revoke>-x[l]\n(Alice)-[edit(Doc)]->(Bob)\n",
    );
    throwsMatching(() => wasm.moveStatement(SOURCE, 2, 0), /can only move to rows/);
  });

  test("deleteStatement deletes, but not what a revocation names", () => {
    assert.equal(wasm.deleteStatement(SOURCE, 2), "rain\nl: (Doc)-[edit(Doc)]->(Alice)\n(Alice)-[edit(Doc)]->(Bob)\n");
    throwsMatching(() => wasm.deleteStatement(SOURCE, 0), /revokes statement 0; delete that first/);
  });

  test("insertStatement adds each kind, and refuses unknown kinds and levels", () => {
    assert.match(wasm.insertStatement(SOURCE, 3, "delegate", 2, 0, "read"), /\(Bob\)-\[read\(Bob\)\]->\(Doc\)/);
    assert.match(wasm.insertStatement(SOURCE, 1, "invoke", 1, 0, "edit"), /\(Alice\)=\[\[edit\]\]=>\(Doc\)/);
    assert.match(wasm.insertStatement(SOURCE, 3, "revoke", 0, 1, ""), /alice-bob: \(Alice\)-\[edit\(Doc\)\]->\(Bob\)[\s\S]*x\[alice-bob\]/);
    // An explicit subject: Alice passes on authority over herself, not Doc
    assert.match(wasm.insertStatement(SOURCE, 1, "delegate", 1, 2, "read", 1), /\(Alice\)-\[read\(Alice\)\]->\(Bob\)/);
    assert.match(wasm.insertStatement(SOURCE, 1, "delegate", 1, 2, "/crud", 0), /\(Alice\)-\[\/crud\(Doc\)\]->\(Bob\)/);
    throwsMatching(() => wasm.insertStatement(SOURCE, 0, "frobnicate", 0, 1, "read"), /unknown statement kind "frobnicate"/);
    throwsMatching(() => wasm.insertStatement(SOURCE, 0, "delegate", 0, 1, "write"), /unknown access level "write"/);
    throwsMatching(() => wasm.insertStatement(SOURCE, 0, "delegate", 9, 1, "read"), /there's no column 9/);
  });

  test("movePrincipal writes a principals: line", () => {
    assert.match(wasm.movePrincipal(SOURCE, 0, 2), /^rain\nprincipals: Alice, Bob, Doc\n/);
    throwsMatching(() => wasm.movePrincipal(SOURCE, 0, 9), /there's no column 9/);
  });

  test("region notes and rows", () => {
    const regioned = "rain\n>>> R: old\n(A)-[read(A)]->(B)\n<<< R\n(B)-[read(A)]->(C)\n";
    assert.match(wasm.setRegionNote(regioned, "R", "new"), />>> R: new/);
    assert.equal(wasm.setRegionRows(regioned, "R", 0, 1), "rain\n>>> R: old\n(A)-[read(A)]->(B)\n(B)-[read(A)]->(C)\n<<< R\n");
    throwsMatching(() => wasm.setRegionNote(regioned, "Z", "x"), /no region called `Z`/);
    throwsMatching(() => wasm.setRegionNote(regioned, "R", "a // b"), /comment marker/);
  });
});

describe("RainDiagram", () => {
  test("kind, events, renders, and moments", () => {
    const d = new wasm.RainDiagram(SOURCE);
    try {
      assert.equal(d.kind, "rain");
      assert.equal(d.events, 3);
      assert.match(d.svg(), /^<svg/);
      assert.match(d.rainSvg(), /^<svg/);
      assert.match(d.certflowSvg(), /certflow/);
      assert.match(d.rainSvgAt(1), /data-moment="1"/);
      assert.match(d.certflowSvgAt(1), /^<svg/);
      assert.match(d.text(), /\(Doc\)/);
    } finally {
      d.free();
    }
  });

  test("queries", () => {
    const d = new wasm.RainDiagram("rain\n(Doc)-[edit(Doc)]->(Alice)\n(Alice)-[read(Doc)]->(Bob)\n");
    try {
      // Bob's own route: the certificate he holds (Doc→Alice powers it, but
      // isn't on his route)
      assert.deepEqual(JSON.parse(d.reach("bob")), { subjects: [{ key: "doc", level: "read", caveats: [] }], events: [1] });
      assert.deepEqual(JSON.parse(d.reach("nobody")), { subjects: [], events: [] });
      assert.deepEqual(Array.from(d.principals()), ["Doc", "Alice", "Bob"]);
      // What Alice (column 1) could pass on at each row
      assert.deepEqual(JSON.parse(d.heldBefore(1, 0)), []);
      assert.deepEqual(JSON.parse(d.heldBefore(1, 1)), [{ subject: 0, level: "edit" }]);
      assert.deepEqual(JSON.parse(d.heldBefore(9, 1)), []);
      // Command paths and caveats, as in UCAN: one entry per broadest ability
      const u = new wasm.RainDiagram("rain\n(Doc)-[/crud(Doc) where path = \"/a\"]->(Alice)\n(Doc)-[/msg(Doc)]->(Alice)\n");
      try {
        assert.deepEqual(JSON.parse(u.reach("Alice")).subjects, [
          { key: "doc", level: "/crud", caveats: ["path = \"/a\""] },
          { key: "doc", level: "/msg", caveats: [] },
        ]);
      } finally {
        u.free();
      }
      assert.deepEqual(Array.from(d.dependents(0)), [1]);
      assert.deepEqual(Array.from(d.dependents(99)), []);
      assert.deepEqual(Array.from(d.movable(1)), [1, 1]);
    } finally {
      d.free();
    }
  });

  test("bad source throws from the constructor", () => {
    throwsMatching(() => new wasm.RainDiagram("rain\n(A)-<revoke>-x[x]"), /no earlier certificate is labelled `x`/);
  });

  test("use after free throws instead of crashing", () => {
    const d = new wasm.RainDiagram(SOURCE);
    d.free();
    assert.throws(() => d.svg());
    assert.equal(typeof d[Symbol.dispose], "function");
  });
});

describe("the JS boundary", () => {
  test("wrong argument types are refused", () => {
    throwsMatching(() => wasm.renderSvg(undefined), /expected a string/);
    throwsMatching(() => wasm.renderSvg(42), /expected a string/);
    // Numbers go through JS coercion first: numeric strings work like
    // numbers, and anything else becomes NaN, which is refused
    assert.equal(wasm.moveStatement(SOURCE, "1", "2"), wasm.moveStatement(SOURCE, 1, 2));
    throwsMatching(() => wasm.moveStatement(SOURCE, "one", 2), /^from must be a whole number ≥ 0, not NaN$/);
  });

  // Numbers are taken as JS numbers and checked, rather than letting
  // wasm-bindgen wrap negatives (-1 → 4294967295) or drop fractions
  test("negative and fractional numbers are refused, naming the argument", () => {
    throwsMatching(() => wasm.setPosition(SOURCE, 1, -5), /^percent must be a whole number ≥ 0, not -5$/);
    throwsMatching(() => wasm.moveStatement(SOURCE, -1, 0), /^from must be a whole number ≥ 0, not -1$/);
    throwsMatching(() => wasm.moveStatement(SOURCE, 1.7, 2), /^from must be a whole number ≥ 0, not 1.7$/);
    throwsMatching(() => wasm.deleteStatement(SOURCE, NaN), /^statement must be a whole number ≥ 0, not NaN$/);
    throwsMatching(() => wasm.insertStatement(SOURCE, Infinity, "invoke", 0, 1, "read"), /^at must be a whole number/);
    throwsMatching(() => wasm.movePrincipal(SOURCE, 0, -2), /^to must be a whole number/);
    throwsMatching(() => wasm.setRegionRows("rain\n>>> R\n(A)-[read(A)]->(B)\n<<< R", "R", 0.5, 0), /^first must be a whole number/);
  });

  test("class methods check their numbers too", () => {
    const d = new wasm.RainDiagram(SOURCE);
    try {
      throwsMatching(() => d.rainSvgAt(-1), /^happened must be a whole number/);
      throwsMatching(() => d.certflowSvgAt(1.5), /^happened must be a whole number/);
      throwsMatching(() => d.dependents(-1), /^event must be a whole number/);
      throwsMatching(() => d.movable(0.1), /^event must be a whole number/);
      // Whole numbers past the end are fine: nothing there
      assert.deepEqual(Array.from(d.dependents(99)), []);
    } finally {
      d.free();
    }
  });

  test("percentages past 100 count as 100", () => {
    assert.equal(wasm.setPosition(SOURCE, 1, 250), wasm.setPosition(SOURCE, 1, 100));
  });

  test("reach accepts a display name as well as a key", () => {
    const d = new wasm.RainDiagram("rain\n(Doc)-[edit(Doc)]->(Alice)\n");
    try {
      assert.deepEqual(JSON.parse(d.reach("Alice")), JSON.parse(d.reach("alice")));
      assert.notDeepEqual(JSON.parse(d.reach("Alice")).subjects, []);
    } finally {
      d.free();
    }
  });
});
