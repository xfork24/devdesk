import { readFileSync } from "node:fs";

const request = JSON.parse(readFileSync(0, "utf8"));
const name = request.params.handler;
const handlers = ["PLACEHOLDER_HANDLER"];
let result;
if (request.method === "describe") {
  result = { api: 1, handlers };
} else if (name.endsWith(".repository.head")) {
  result = { head: request.params.core.head, observedBy: "domain-worker" };
} else {
  result = { title: String(request.params.input.title ?? "").trim(), observedBy: "domain-worker" };
}
process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }));
