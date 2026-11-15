// Trusted SDK extension: consume the same immutable Permission used by Docker.
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
export const name = 'devdesk-tool-boundary';
export const inject = ['tools', 'agents', 'workingDirectory', 'loader'];
export async function apply(ctx, config = {}) {
  const plan = JSON.parse(readFileSync('/run/devdesk/permission', 'utf8'));
  const policy = plan.harness.policy.tools;
  ctx.on('tools/execute', async (execution, next) => {
    // Bridged operations are checked against their exact frozen Tool refs by the Host.
    if ((config.dynamicTools || []).some((tool) => tool.name === execution.name)) return next();
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
  if (!config.contextBridge) return;
  const require = createRequire('/usr/local/lib/node_modules/@deepseek-ai/dsh/package.json');
  const { JsonRpcLineTransport } = await import(require.resolve('@deepseek-ai/dsh-sdk-protocol'));
  const { HarnessSdkJsonRpcServer } = await import(require.resolve('@deepseek-ai/dsh-sdk-jsonrpc-server'));
  const transport = new JsonRpcLineTransport(process.stdin, process.stdout);
  const server = new HarnessSdkJsonRpcServer(ctx, transport, { maxTokensAsSuccess: false });
  ctx.provide('sdkSessionActivation', { ready: true });
  for (const tool of config.dynamicTools || []) {
    ctx.tools.register({
      name: tool.name, description: tool.description, parameters: tool.inputSchema,
      output: { schema: {}, render: (_args, result) => [{ type: 'text', text: JSON.stringify(result) }] },
      async execute(args, execution) {
        if (!execution.agent) throw new Error('Agent context requires an owning session.');
        const receipt = await transport.request('devdesk/tool/call', {
          sessionId: String(execution.agent.session.id), callId: String(execution.callId),
          tool: tool.name, arguments: args,
        }, execution.signal);
        if (!receipt.success) throw Object.assign(new Error(receipt.error?.message || 'DevDesk tool call failed'), { code: receipt.error?.code });
        return receipt.structured_content ?? receipt.content;
      },
    });
  }
  transport.onRequest(async (method, params) => {
    if (method === 'initialize') await ctx.get('loader')?.await();
    const result = await server.handleRequest(method, params);
    if (method === 'shutdown') setImmediate(async () => {
      await transport.flush();
      await ctx.root.fiber.dispose();
      process.exit(0);
    });
    return result;
  });
  ctx.effect(() => {
    transport.start();
    return async () => { await server.shutdown(); transport.close(); };
  }, 'devdesk.context-jsonrpc');
}
