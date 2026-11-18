import { QueryCache } from "./query-cache";
import type { RequestOptions, Row } from "./components/rpc-types";

type Change = { project: string; instance?: string; root?: string };
const listeners = new Set<(change: Change) => void>();
const generations = new Map<string, number>();
let nextGeneration = 0;
const cache = new QueryCache(128, 8 * 1024 * 1024);
const reads = new Set([
  "query.execution_workspace",
  "query.workflow_progress",
  "query.workflow_management_summary",
  "query.orchestration",
  "query.workflow_driver",
]);

export function observeRuntimeCommand(method: string, params: Row) {
  if (
    !/^(workflow\.|orchestration\.|manager\.|tool\.call\.|interaction\.|context\.update|feature\.|task\.)/.test(
      method,
    )
  )
    return;
  const project = params.project_id || "";
  generations.set(project, ++nextGeneration);
  while (generations.size > 64)
    generations.delete(generations.keys().next().value!);
  for (const listener of listeners) listener({ project });
}

// Project cursors can advance for another root. They invalidate reads but do
// not change the displayed facts or any command's original resource version.
export function runtimeDisplayKey(value: unknown): string {
  if (!value || typeof value !== "object") return JSON.stringify(value) || "";
  const row = value as Row;
  return JSON.stringify({
    ...row,
    cursor: undefined,
    ...(row.progress
      ? { progress: { ...row.progress, cursor: undefined } }
      : {}),
  });
}

export function observeRuntimeEvent(project: string, event: Row) {
  let change: Change | undefined;
  if (event.kind === "workflow.runtime") {
    const source = JSON.parse(event.detail);
    change = {
      project,
      instance: source.instance_id,
      root: source.root_instance_id,
    };
  } else if (
    /^(workflow\.|orchestration\.|manager\.|runtime\.controller\.)/.test(
      event.kind || "",
    ) ||
    ["stream.ready", "stream.reset"].includes(event.kind)
  ) {
    change = { project };
  } else if (event.kind === "resource.changed") {
    const versions: Row[] = JSON.parse(event.detail);
    // Token and tool-journal appends do not invalidate an entire execution UI.
    if (
      versions.some(({ resource }) =>
        ["feature", "task", "project", "interaction"].includes(resource?.kind),
      )
    )
      change = { project };
  }
  if (!change) return;
  generations.set(project, ++nextGeneration);
  while (generations.size > 64)
    generations.delete(generations.keys().next().value!);
  for (const listener of listeners) listener(change);
}

export function subscribeRuntime(
  project: string,
  instance: string,
  root: boolean,
  refresh: () => void,
) {
  const listener = (change: Change) => {
    if (
      change.project === project &&
      (!change.instance ||
        change.instance === instance ||
        (root && change.root === instance))
    )
      refresh();
  };
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function readRuntime<T>(
  method: string,
  params: Row,
  load: (signal: AbortSignal) => Promise<T>,
  options: RequestOptions,
  host: string,
): Promise<T> {
  const eligible =
    reads.has(method) ||
    (method === "query.workflow_instance" && params.view === "status");
  if (!eligible || options.fresh)
    return load(options.signal || new AbortController().signal);
  const project = params.project_id || "";
  const key = JSON.stringify([
    host,
    project,
    generations.get(project) || 0,
    method,
    params,
  ]);
  // Generation changes never cancel another consumer's admitted read. The
  // owning component coalesces a follow-up and fences obsolete responses.
  return cache.read(key, load, options.signal, 150);
}
