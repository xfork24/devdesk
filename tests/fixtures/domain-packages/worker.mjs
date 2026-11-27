import { readFileSync } from "node:fs";

const request = JSON.parse(readFileSync(0, "utf8"));
const name = request.params.handler;
const handlers = ["PLACEHOLDER_HANDLER"];
let result;
if (request.method === "describe") {
  result = { api: 1, handlers };
} else if (name.endsWith(".repository.head")) {
  result = { head: request.params.core.head, observedBy: "domain-worker" };
} else if (name.endsWith(".artifact.create")) {
  result = { kind: "artifact", title: String(request.params.input.title ?? "").trim() };
} else if (name.endsWith(".inventory.read")) {
  const titles = request.params.core.entities.map((entity) => entity.data.title.trim()).sort();
  result = { title: titles.join("\n"), observedBy: "domain-worker" };
} else if (name.endsWith(".documents.list")) {
  // The kernel input domain.entities: the documents of the project the handler runs in.
  const titles = request.params.core.entities.map((entity) => entity.data.title);
  result = { title: titles.join("\n"), titles, truncated: request.params.core.truncated,
             observedBy: "domain-worker" };
} else {
  result = { title: String(request.params.input.title ?? "").trim(), observedBy: "domain-worker" };
}
process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }));
