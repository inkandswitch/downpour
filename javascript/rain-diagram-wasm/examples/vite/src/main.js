// Nothing Wasm-specific to configure: Vite 8 emits the .wasm as an asset and
// the package loads it.
import { RainDiagram, renderSvg } from "@inkandswitch/rain-diagram-wasm";

const source = `rain
da: (Doc)-[edit(Doc)]->(Alice)
(Alice)-[edit(Doc)]->(Bob)
(Doc)-<revoke>-x[da]`;

const status = document.getElementById("status");
try {
  const diagram = new RainDiagram(source);
  document.getElementById("picture").innerHTML = renderSvg(source);
  // The check (`check.mjs`) looks for this exact line
  status.textContent = `rendered ${diagram.events} events`;
  diagram.free();
} catch (error) {
  status.textContent = `failed: ${error}`;
}
