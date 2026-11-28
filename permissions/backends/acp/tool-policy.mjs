import { lstatSync, readFileSync, realpathSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const denial = 'The frozen DevDesk Permission denies this tool or path.';

export function readPolicy() {
  const permission = JSON.parse(readFileSync('/run/devdesk/permission', 'utf8'));
  if (!permission.harness?.policy || !permission.sandbox?.policy || !permission.scope) {
    throw new Error('Missing frozen DevDesk Permission');
  }
  return permission;
}

function canonical(path) {
  try {
    return realpathSync(path);
  } catch (error) {
    if (error.code !== 'ENOENT' || dirname(path) === path) throw error;
    try {
      lstatSync(path);
      throw new Error('Unresolvable symbolic link');
    } catch (missing) {
      if (missing.code !== 'ENOENT') throw missing;
    }
    return join(canonical(dirname(path)), basename(path));
  }
}

const within = (path, root) => path === root || path.startsWith(root + '/');

export function permitted(permission, name, input) {
  if (typeof name !== 'string') return false;
  const alias = name.toLowerCase();
  // The host listener authenticates these requests and core checks the exact frozen tool.
  if (alias.startsWith('devdesk_') || alias.startsWith('mcp__devdesk__')) return true;
  const policy = permission.harness.policy;
  if (policy.tools.mode === 'restricted') {
    const allow = policy.tools.allow.some((tool) => tool === name || tool.toLowerCase() === alias);
    const deny = policy.tools.deny.some((tool) => tool === name || tool.toLowerCase() === alias);
    if (!allow || deny) return false;
  }
  const full = [policy, permission.sandbox.policy].every((layer) => layer.filesystem.mode === 'full_access');
  if (alias === 'shell' || alias === 'bash') {
    return full && permission.isolation.credential_boundary === 'container-trust';
  }
  const write = ['edit', 'write'].includes(alias);
  if (!write && !['read', 'glob', 'grep', 'ls'].includes(alias)) return false;
  const target = input?.path ?? input?.filePath ?? input?.file_path
    ?? (['glob', 'grep', 'ls'].includes(alias) ? '/work' : undefined);
  if (typeof target !== 'string' || target.startsWith('file:') || target.includes('\0')) return false;
  try {
    const path = canonical(resolve('/work', target));
    if (!['/work', '/skills'].some((root) => within(path, root))) return false;
    if (!write) return true;
    if (!within(path, '/work') || [policy, permission.sandbox.policy].some((p) => p.filesystem.mode === 'read_only')) return false;
    if (permission.scope.read_only_paths.some((root) => within(path, resolve('/work', root)))) return false;
    return full || permission.scope.writable_paths.some((root) => within(path, resolve('/work', root)));
  } catch {
    return false;
  }
}

async function hook() {
  const permission = readPolicy();
  if (process.argv[2] === '--probe') return;
  let data = '';
  for await (const chunk of process.stdin) {
    data += chunk;
    if (data.length > 1024 * 1024) throw new Error('Hook input exceeds policy budget');
  }
  const event = JSON.parse(data);
  if (event.hook_event_name !== 'PreToolUse' || !permitted(permission, event.tool_name, event.tool_input)) {
    console.log(JSON.stringify({ hookSpecificOutput: {
      hookEventName: 'PreToolUse', permissionDecision: 'deny', permissionDecisionReason: denial,
    } }));
  }
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    await hook();
  } catch (error) {
    // MCode treats exit 2 as an explicit tool refusal.
    console.error(denial);
    process.exitCode = 2;
  }
}
