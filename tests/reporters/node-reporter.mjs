// Trusted reporter: test stdout is data, never a report record. No raw environment or errors are exported.
export default async function* reporter(source) {
  for await (const event of source) {
    if (event.type === 'test:fail') {
      const codes = []; let error = event.data.details?.error;
      for (let depth = 0; error && depth < 8; depth++, error = error.cause) codes.push(error.code || error.name || 'unknown');
      const classification = codes.includes('ERR_ASSERTION') || codes.includes('AssertionError') ? 'assertion' : event.data.details?.error?.failureType === 'subtestsFailed' ? 'aggregate' : 'environment';
      yield JSON.stringify({devdesk_test_report:1,event:'fail',classification,codes})+'\n';
    }
    if (event.type === 'test:summary') yield JSON.stringify({devdesk_test_report:1,event:'summary',counts:event.data.counts})+'\n';
  }
}
