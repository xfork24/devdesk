import { writeFileSync } from 'node:fs';
import { denial, permitted, readPolicy } from '/run/devdesk/tool-policy.mjs';

export default {
  id: 'devdesk-frozen-permission',
  async setup(ctx) {
    const permission = readPolicy();
    await ctx.tool.hook('execute.before', (event) => {
      if (!permitted(permission, event.tool, event.input)) throw new Error(denial);
    });
    writeFileSync(process.env.DEVDESK_POLICY_MARKER, 'devdesk.acp.policy.v1', { flag: 'wx', mode: 0o600 });
  },
};
