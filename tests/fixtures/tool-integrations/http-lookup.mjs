// Standard HTTP parser and explicit request count; no external service or model.
import assert from 'node:assert/strict';
import http from 'node:http';
import { createInterface } from 'node:readline';
let count = 0;
const server = http.createServer(async (request, response) => {
  for await (const _ of request) {}
  assert.equal(request.method, 'GET');
  assert.equal(request.url, '/tickets/A-1');
  count++;
  response.writeHead(200, { 'Content-Type': 'application/json' });
  response.end(JSON.stringify({ ok: true, count }));
});
server.listen(0, '0.0.0.0', () => console.log(JSON.stringify({ port: server.address().port })));
createInterface({ input: process.stdin }).on('line', command => {
  assert.equal(command, 'count');
  console.log(JSON.stringify({ count }));
});
