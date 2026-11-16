import {test} from 'node:test';
import assert from 'node:assert/strict';
import {QueryCache} from '../../web/src/query-cache.ts';
const tick=()=>new Promise(r=>setTimeout(r,0));
test('consumers coalesce; one abort leaves the other alive; last abort cancels transport', async()=>{
 const cache=new QueryCache(); let calls=0, transport, finish;
 const load=signal=>{calls++;transport=signal;return new Promise(r=>finish=r);};
 const a=new AbortController(), b=new AbortController();
 const first=cache.read('k',load,a.signal);const second=cache.read('k',load,b.signal);
 await tick();a.abort();await assert.rejects(first,{name:'AbortError'});assert.equal(transport.aborted,false);
 finish({n:1});assert.deepEqual(await second,{n:1});assert.equal(calls,1);
 await cache.read('k',load);assert.equal(calls,1);
 const c=new AbortController();const pending=cache.read('other',load,c.signal);await tick();c.abort();
 await assert.rejects(pending,{name:'AbortError'});assert.equal(transport.aborted,true);finish({n:2});await tick();
 assert.equal(cache.peek('other'),undefined);
});
test('failure retry, TTL, invalidation and byte/entry eviction',async()=>{
 const cache=new QueryCache(2,100);let calls=0;
 await assert.rejects(cache.read('bad',async()=>{throw Error('offline');}));
 assert.equal(await cache.read('bad',async()=>42),42);
 await cache.read('a',async()=>++calls,undefined,1);await new Promise(r=>setTimeout(r,5));
 await cache.read('a',async()=>++calls);assert.equal(calls,2);
 await cache.read('b',async()=>3);assert.equal(cache.peek('bad'),undefined);
 await cache.read('large',async()=> 'x'.repeat(200));assert.equal(cache.peek('large'),undefined);
 cache.clear();assert.equal(cache.peek('b'),undefined);
});
test('a cleared response cannot repopulate a new identity',async()=>{
 const cache=new QueryCache();let finish;
 const pending=cache.read('same',()=>new Promise(r=>finish=r));await tick();cache.clear();
 const next=cache.read('same',async()=> 'new');finish('old');
 await assert.rejects(pending,{name:'AbortError'});assert.equal(await next,'new');assert.equal(cache.peek('same'),'new');
});
