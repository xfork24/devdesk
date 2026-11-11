// Trusted reporter: isolated test diagnostics are data, never classification records.
// Error.message/stack/cause are non-enumerable; retain them without invoking getters.
function diagnostic(value, seen = new WeakSet()) {
  if (value === null || ['string', 'number', 'boolean'].includes(typeof value)) return value;
  if (typeof value !== 'object') return String(value);
  if (seen.has(value)) return '[Circular]';
  seen.add(value);
  const result = Array.isArray(value) ? [] : Object.create(null);
  if (value instanceof Error) {
    const name = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(value), 'name');
    if (name && 'value' in name) result.name = diagnostic(name.value, seen);
  }
  for (const [key, descriptor] of Object.entries(Object.getOwnPropertyDescriptors(value))) {
    if (key === 'length' && Array.isArray(value)) continue;
    result[key] = 'value' in descriptor ? diagnostic(descriptor.value, seen) : '[Accessor]';
  }
  seen.delete(value);
  return result;
}
export default async function* reporter(source) {
  for await (const event of source) {
    if (event.type === 'test:fail') {
      const codes = []; let error = event.data.details?.error;
      for (let depth = 0; error && depth < 8; depth++, error = error.cause) codes.push(error.code || error.name || 'unknown');
      const classification = codes.includes('ERR_ASSERTION') || codes.includes('AssertionError') ? 'assertion' : event.data.details?.error?.failureType === 'subtestsFailed' ? 'aggregate' : 'environment';
      yield JSON.stringify({devdesk_test_report:1,event:'fail',classification,codes,diagnostic:diagnostic(event.data)})+'\n';
    }
    if (['test:stdout', 'test:stderr', 'test:diagnostic'].includes(event.type)) yield JSON.stringify({devdesk_test_report:1,event:'diagnostic',source:event.type,diagnostic:diagnostic(event.data)})+'\n';
    if (event.type === 'test:summary') yield JSON.stringify({devdesk_test_report:1,event:'summary',counts:event.data.counts})+'\n';
  }
}
