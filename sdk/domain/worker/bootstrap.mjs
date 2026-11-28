// Kernel-owned bootstrap of a domain worker. It is the process the sandbox runs: it reads
// the one request, loads the package entry as a module, calls a handler and writes the reply.
// A package entry only exports its handlers and does no I/O of its own:
//
//   export const handlers = { "package.handler": (input, core) => value };
//
// A handler refuses work by throwing an Error that carries a `code` of its own. The Host
// answers the caller with that code and message as `domain_handler_rejected`. Anything else
// that goes wrong here is a worker failure, which the Host reports as `domain_worker_failed`.
import { readFileSync } from "node:fs";

const entry = process.argv[2] ?? "/entry.mjs";
const API = 2;
const CODE = /^[a-z][a-z0-9_]{0,63}$/;
const describe = (error) =>
  error instanceof Error ? error.message : String(error);
const reply = (id, result) => JSON.stringify({ jsonrpc: "2.0", id, result });
const failure = (id, code, message, refused = false) =>
  JSON.stringify({
    jsonrpc: "2.0",
    id,
    error: {
      code: -32000,
      message,
      data: { code, ...(refused ? { refused: true } : {}) },
    },
  });

let request = null;
let output;
try {
  request = JSON.parse(readFileSync(0, "utf8"));
  const { handlers } = await import(entry);
  if (
    handlers === null ||
    typeof handlers !== "object" ||
    Array.isArray(handlers)
  ) {
    throw new Error("the entry does not export a handlers table");
  }
  if (request.method === "describe") {
    output = reply(request.id, {
      api: API,
      handlers: Object.keys(handlers).sort(),
    });
  } else if (request.method === "invoke") {
    const { handler, input, core } = request.params ?? {};
    if (!Object.hasOwn(handlers, handler))
      throw new Error(`the entry has no handler ${handler}`);
    if (typeof handlers[handler] !== "function") {
      throw new Error(`handler ${handler} is not a function`);
    }
    try {
      output = reply(request.id, await handlers[handler](input, core));
    } catch (error) {
      output = failure(
        request.id,
        CODE.test(error?.code ?? "") ? error.code : "handler_failed",
        describe(error),
        true,
      );
    }
  } else {
    throw new Error(`unknown method ${request.method}`);
  }
} catch (error) {
  output = failure(request?.id, "worker_failed", describe(error));
}
process.stdout.write(output);
