// A stand-in for Patchwork, for tests: the globals a tool reads (`repo`,
// `accountDocHandle`), a document made by the datatype, and the tool
// mounted twice through the render contract. No network: one repo, in memory.

import "@automerge/automerge";
import { createMemorySigner } from "@automerge/subduction";
import { Repo } from "@automerge/automerge-repo";
import { SubductionBackend } from "@automerge/automerge-repo-subduction";
import { DummySubductionStorageAdapter } from "@automerge/automerge-repo-subduction/helpers/DummySubductionStorageAdapter.js";
import { plugins } from "../index.js";

const backend = new SubductionBackend({ storage: new DummySubductionStorageAdapter(), network: [], signer: createMemorySigner() });
const repo = (window.repo = new Repo({ backend }));
const contact = await repo.create({ name: "Ada Lovelace" });
window.accountDocHandle = await repo.create({ contactUrl: contact.url });

const [datatype, tool] = await Promise.all(plugins.map((plugin) => plugin.load()));
const handle = await repo.create({});
await handle.change((doc) => datatype.init(doc));

window.handle = handle;
window.datatype = datatype;
window.cleanups = ["one", "two"].map((id) => tool(handle, document.getElementById(id)));
