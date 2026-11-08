// Public Cordis tools/execute extension, loaded by the supported dsh SDK profile.
// File access uses the native bwrap-backed bash tool. Raw in-process filesystem,
// plugin, PTC and process tools cannot read the harness's credential environment.
export const name = 'devdesk-tool-boundary';
export const inject = ['tools'];
const allowed = new Set(['bash', 'ask_user_question']);
export function apply(ctx) {
  ctx.on('tools/execute', async (execution, next) => {
    const args = execution.arguments;
    if (!allowed.has(execution.name) ||
        (execution.name === 'bash' && args && typeof args === 'object' && args.sandbox_permissions != null)) {
      const message = 'This operation is outside the frozen DevDesk tool policy. Use the confined bash workspace tools or ask the user.';
      return { content: [{ type: 'text', text: message }], isError: true,
        error: { message, info: { name: 'DevDeskToolPolicy', code: 'DEVDESK_TOOL_POLICY' } } };
    }
    return next();
  });
}
