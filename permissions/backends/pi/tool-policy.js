import { lstatSync, readFileSync, realpathSync } from 'node:fs';
import { basename, dirname, join, resolve, sep } from 'node:path';
// Use the pinned CLI's actual resolver, including the read tool's filename
// fallbacks, so the hook checks the same target that the tool will open.
import { resolveReadPath, resolveToCwd } from '/usr/local/lib/node_modules/@earendil-works/pi-coding-agent/dist/core/tools/path-utils.js';

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function canonicalPath(absolute) {
  try {
    lstatSync(absolute);
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    const parent = dirname(absolute);
    if (parent === absolute) throw error;
    return join(canonicalPath(parent), basename(absolute));
  }
  return realpathSync(absolute);
}

function within(path, parent) {
  return path === parent || path.startsWith(parent + sep);
}

const denied = { block: true, reason: 'The frozen DevDesk Permission denies this tool or path.' };

export default function devdesk(pi) {
  const permission = readJson('/run/devdesk/permission');
  const policy = permission.harness.policy;
  const bothFullAccess = [policy, permission.sandbox.policy].every(
    (layer) => layer.filesystem.mode === 'full_access',
  );
  const definitions = readJson('/run/devdesk/dynamic-tools.json');
  const bridged = new Set(definitions.map((tool) => tool.name));
  const writable = permission.scope.writable_paths.map((path) => resolve('/work', path));
  const readOnly = permission.scope.read_only_paths.map((path) => resolve('/work', path));

  pi.on('session_start', (_event, ctx) => {
    ctx.ui.notify('devdesk.policy.loaded.v1', 'info');
  });

  pi.on('tool_call', async (event, ctx) => {
    const name = event.toolName;
    if (bridged.has(name)) return;
    if (policy.tools.mode === 'restricted' &&
      (policy.tools.deny.includes(name) || !policy.tools.allow.includes(name))) return denied;

    if (name === 'bash') {
      if (!bothFullAccess || permission.isolation.credential_boundary !== 'container-trust') return denied;
    } else if (['read', 'edit', 'write'].includes(name)) {
      const rawPath = event.input?.path;
      if (typeof rawPath !== 'string') return denied;
      let path;
      try {
        if (ctx.cwd !== '/work') return denied;
        const resolved = name === 'read' ? resolveReadPath(rawPath, ctx.cwd) : resolveToCwd(rawPath, ctx.cwd);
        path = canonicalPath(resolved);
      } catch {
        return denied;
      }
      if (!['/work', '/skills', '/tmp/devdesk-runtime' ].some((root) => within(path, root))) return denied;
      if (name !== 'read') {
        if ([policy, permission.sandbox.policy].some((layer) => layer.filesystem.mode === 'read_only')) return denied;
        if (!within(path, '/work') || readOnly.some((root) => within(path, root))) return denied;
        if (!writable.some((root) => within(path, root))) return denied;
      }
    } else {
      return denied;
    }

    if (name !== 'read' && permission.approval === 'on_request') {
      const result = await ctx.ui.select(`devdesk.approval.v1 ${name}`, ['Allow once', 'Reject']);
      if (result !== 'Allow once') return denied;
    }
  });

  for (const tool of definitions) {
    pi.registerTool({
      name: tool.name,
      label: tool.name,
      description: tool.description,
      parameters: tool.inputSchema,
      async execute(id, arguments_, _signal, _progress, ctx) {
        const input = JSON.stringify({ tool: tool.name, call: id, arguments: arguments_ });
        const value = await ctx.ui.input(`devdesk.tool_call.v1 ${input}`, '');
        const response = value === undefined ? { success: false, error: 'Tool call cancelled' } : JSON.parse(value);
        const content = (response.content ?? []).map((item) => {
          if (item.type === 'text' || item.type === 'image') return item;
          return { type: 'text', text: JSON.stringify(item) };
        });
        if (response.structured_content !== undefined && response.structured_content !== null) {
          content.push({ type: 'text', text: JSON.stringify(response.structured_content) });
        }
        if (!response.success) content.push({ type: 'text', text: JSON.stringify(response.error ?? 'Tool call failed') });
        return { content, details: { success: response.success } };
      },
    });
  }
}
