// The preview's Tour example is the `tour.rain` golden, so the golden's
// rendering checks cover what the Tour shows. `node --test` (or `nix run .#ci-js`).

import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";

const read = (relative) => fs.readFileSync(new URL(relative, import.meta.url), "utf8");

/** The text of the template literal `const NAME = \`…\`;` in `source`. */
const literal = (source, name) => {
  const found = source.match(new RegExp(`^const ${name} = \`([\\s\\S]*?)^\`;`, "m"));
  assert.ok(found, `main.js defines ${name} as a template literal`);
  const body = found[1];
  // Interpolation would make the text depend on the page, not the file
  assert.ok(!/(^|[^\\])\$\{/.test(body), `${name} has no \${…} interpolation`);
  return body.replace(/\\([`\\$])/g, "$1");
};

test("the preview's Tour is the tour.rain golden", () => {
  const tour = literal(read("./main.js"), "TOUR");
  const golden = read("../../rust/rain_diagram_core/tests/golden/tour.rain");
  assert.equal(tour, golden, "edit both together: javascript/preview/main.js and tests/golden/tour.rain");
});
