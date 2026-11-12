import init, { toSiteJson } from "./pkg/rain_diagram_wasm.js";

const EXAMPLE = `rain
title Revoking two hops downstream

(Doc)-[edit]->(Alice)            : The Doc grants Alice Edit
(Alice)-[edit(Doc)]->(Bob)       : Alice passes Edit to Bob
(Bob)=[[edit]]=>(Doc)            : Bob edits the Doc
(Bob)-[read]->(Carol)            : Bob gives Carol Read
cd: (Carol)-[read]->(Dan)        : Carol shares Read with Dan
(Alice)-<revoke>-x[cd]           : Alice revokes Carol→Dan
`;

const STORAGE_KEY = "rain-preview:source";

const source = document.getElementById("source");
const status = document.getElementById("status");
const json = document.getElementById("json");

const load = () => {
  try {
    return localStorage.getItem(STORAGE_KEY) ?? EXAMPLE;
  } catch {
    return EXAMPLE;
  }
};

const save = (text) => {
  try {
    localStorage.setItem(STORAGE_KEY, text);
  } catch {
    // Storage is a convenience; the preview works without it
  }
};

const render = () => {
  save(source.value);

  try {
    const site = JSON.parse(toSiteJson(source.value));
    status.className = "ok";
    status.textContent = `✓ ${site.agents.length} principals, ${site.events.length} events`;
    json.textContent = JSON.stringify(site, null, 2);
  } catch (e) {
    status.className = "err";
    status.textContent = `✗ ${e.message ?? e}`;
  }
};

// Sent by the dev server (serve.mjs) when the Wasm rebuild fails; the page
// keeps running the last good build
window.addEventListener("rain:build-error", ({ detail }) => {
  status.className = "err";
  status.textContent = "✗ Wasm build failed; still running the last good build";
  json.textContent = `${detail}\n\n${json.textContent}`;
});

await init();
source.value = load();
source.addEventListener("input", render);
render();
