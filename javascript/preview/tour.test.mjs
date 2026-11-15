// The preview's Tour example is the `tour.rain` golden, so the golden's
// rendering checks cover what the Tour shows. `node --test` (or `nix run .#ci-js`).

import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import { TOUR } from "./examples.js";

test("the preview's Tour is the tour.rain golden", () => {
  const golden = fs.readFileSync(new URL("../../rust/rain_diagram_core/tests/golden/tour.rain", import.meta.url), "utf8");
  assert.equal(TOUR, golden, "edit both together: javascript/preview/examples.js and tests/golden/tour.rain");
});
