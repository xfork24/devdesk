// ZCode interprets exit 2 as a blocking PreToolUse result.
import { readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

try {
  const policy = JSON.parse(readFileSync('/run/devdesk/permission', 'utf8'));
  if (!policy.harness?.policy || !policy.sandbox?.policy || !policy.scope) throw Error();
  if (process.argv[2] === '--loaded') {
    writeFileSync(process.argv[3], 'loaded', { mode: 0o600 });
  } else {
    let raw = '';
    for await (const chunk of process.stdin) {
      raw += chunk;
      if (raw.length > 1024 * 1024) throw Error();
    }
    const event = JSON.parse(raw);
    const tool = event.tool_name?.toLowerCase();
    if (event.hook_event_name !== 'PreToolUse') throw Error();
    // The host enforces exact capability, session and turn scope of bridge tools.
    if (!tool?.startsWith('mcp__devdesk__')) {
      if (!['read', 'glob', 'grep', 'ls'].includes(tool)) throw Error();
      const allowed = policy.harness.policy.tools;
      if (allowed.mode === 'restricted' && (!allowed.allow.some((name) => name.toLowerCase() === tool)
          || allowed.deny.some((name) => name.toLowerCase() === tool))) throw Error();
      const target = event.tool_input?.path ?? event.tool_input?.file_path ?? event.tool_input?.filePath;
      if (tool === 'read' && typeof target !== 'string') throw Error();
      const path = realpathSync(resolve('/work', target ?? '/work'));
      if (!['/work', '/skills'].some((directory) => path === directory || path.startsWith(directory + '/'))) throw Error();
    }
  }
} catch {
  console.error('The frozen DevDesk Permission denies this ZCode tool or path.');
  process.exitCode = 2;
}
