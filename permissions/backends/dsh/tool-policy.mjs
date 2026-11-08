// Trusted SDK extension: consume the same immutable Permission used by Docker.
import { readFileSync } from 'node:fs';
export const name = 'devdesk-tool-boundary';
export const inject = ['tools'];
export function apply(ctx) {
  const plan = JSON.parse(readFileSync('/run/devdesk/permission', 'utf8'));
  const policy = plan.harness.policy.tools;
  ctx.on('tools/execute', async (execution, next) => {
    const allowed = policy.mode === 'all_available' ||
      (policy.allow.includes(execution.name) && !policy.deny.includes(execution.name));
    const escalation = execution.arguments?.sandbox_permissions;
    if (!allowed || (plan.approval === 'never' && escalation != null)) {
      const message = 'The frozen DevDesk Permission denies this tool or escalation.';
      return { content: [{ type: 'text', text: message }], isError: true,
        error: { message, info: { name: 'DevDeskToolPolicy', code: 'permission_denied' } } };
    }
    return next();
  });
}
