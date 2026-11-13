// Check the README's "Size" table against the files `nix run .#bodge` just
// built: raw, gzip (level 9), and Brotli (quality 11), each within
// TOLERANCE_KB. `--update` rewrites the table instead.
//
//   node javascript/rain-diagram-wasm/size.mjs [--update]
//
// Run from the repo root after `nix run .#bodge`; `nix run .#ci-package` does both.

import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

const TOLERANCE_KB = 3;
const root = path.resolve(import.meta.dirname, "../..");
const readmePath = path.join(root, "README.md");

// Each row of the table, by how its first cell starts, and the file it measures
const ROWS = [
  ["Core, as Wasm", "javascript/rain-diagram-wasm/dist/rain-diagram-wasm.wasm"],
  ["Its JS bindings", "javascript/rain-diagram-wasm/dist/wasm_bindgen/web/rain_diagram_wasm.js"],
  ["`<rain-diagram>` component", "javascript/rain-diagram/rain-diagram.js"],
  ["Core with the Wasm inlined", "javascript/rain-diagram-wasm/dist/iife/index.js"],
];

const kb = (bytes) => Math.round(bytes / 1000);

function measure(file) {
  const bytes = fs.readFileSync(path.join(root, file));
  return {
    raw: kb(bytes.length),
    gzip: kb(zlib.gzipSync(bytes, { level: 9 }).length),
    brotli: kb(
      zlib.brotliCompressSync(bytes, {
        params: { [zlib.constants.BROTLI_PARAM_QUALITY]: 11, [zlib.constants.BROTLI_PARAM_SIZE_HINT]: bytes.length },
      }).length,
    ),
  };
}

const update = process.argv.includes("--update");
let readme = fs.readFileSync(readmePath, "utf8");
const problems = [];

for (const [label, file] of ROWS) {
  const row = readme.split("\n").find((line) => line.startsWith(`| ${label}`));
  if (!row) {
    problems.push(`README.md has no Size row starting "${label}"`);
    continue;
  }
  const cells = row.split("|").slice(1, -1).map((c) => c.trim());
  const actual = measure(file);
  const columns = ["raw", "gzip", "brotli"];
  if (update) {
    const fresh = [cells[0], ...columns.map((c) => `${actual[c]} KB`)];
    readme = readme.replace(row, `| ${fresh.join(" | ")} |`);
    continue;
  }
  columns.forEach((column, i) => {
    const stated = Number.parseInt(cells[i + 1], 10);
    if (!Number.isFinite(stated) || Math.abs(stated - actual[column]) > TOLERANCE_KB) {
      problems.push(`${label}: README says ${cells[i + 1] || "nothing"} ${column}, it's ${actual[column]} KB (${file})`);
    }
  });
}

if (update) {
  fs.writeFileSync(readmePath, readme);
  console.log("README.md's Size table updated");
} else if (problems.length) {
  console.error(problems.join("\n"));
  console.error(`\nUpdate it with: node javascript/rain-diagram-wasm/size.mjs --update`);
  process.exit(1);
} else {
  console.log(`README.md's Size table is within ${TOLERANCE_KB} KB of the build`);
}
