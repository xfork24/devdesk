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
test('bounded background catalogs survive a detached page without restarting discovery', async()=>{
 const cache=new QueryCache(1,100,true);let finish,calls=0,transport;
 const controller=new AbortController();
 const load=signal=>{calls++;transport=signal;return new Promise(r=>finish=r);};
 const first=cache.read('account',load,controller.signal);await tick();controller.abort();
 await assert.rejects(first,{name:'AbortError'});assert.equal(transport.aborted,false);
 await assert.rejects(cache.read('other-account',load),/繁忙/);
 const second=cache.read('account',load);finish({models:['one']});
 assert.deepEqual(await second,{models:['one']});assert.equal(calls,1);
});

// Exercise the browser-independent version protocol with Node's pinned TypeScript stripper.
const {readFileSync}=await import('node:fs');
const {stripTypeScriptTypes}=await import('node:module');
const clientSource=stripTypeScriptTypes(readFileSync(new URL('../../web/src/configuration-client.ts',import.meta.url),'utf8'))
 .replace('"./query-cache"',JSON.stringify(new URL('../../web/src/query-cache.ts',import.meta.url).href));
const client=await import('data:text/javascript,'+encodeURIComponent(clientSource));
test('configuration keys isolate projects/datasets/hosts and reject late older source clocks',async()=>{
 const version={host:'one',global:10,project:4,project_sources:'sources-4',source_epochs:{p:4}};
 client.observeConfigurationResponse({data_root:'dataset-one',catalog_version:version},{project_id:'p'});
 let calls=0;const load=async()=>++calls;
 assert.equal(await client.readConfiguration('query.configuration_index',{project_id:'p',kind:'role'},load),1);
 assert.equal(await client.readConfiguration('query.configuration_index',{kind:'role',project_id:'p'},load),1);
 assert.equal(await client.readConfiguration('query.configuration_index',{project_id:'q',kind:'role'},load),2);
 assert.equal(client.configurationResponseOutdated({catalog_version:{...version,global:9}},{}),true);
 assert.equal(client.configurationResponseOutdated({catalog_version:{...version,project:3}},{project_id:'p'}),true);
 assert.equal(client.configurationResponseOutdated({catalog_version:{...version,source_epochs:{p:3}}},{}),true);
 client.observeConfigurationVersion({...version,global:11,project:5,project_sources:'sources-5',source_epochs:{p:5}},'p');
 assert.equal(await client.readConfiguration('query.configuration_index',{project_id:'p',kind:'role'},load),3);
 client.observeConfigurationVersion(version,'p');
 assert.equal(await client.readConfiguration('query.configuration_index',{project_id:'p',kind:'role'},load),3);
 client.observeConfigurationResponse({data_root:'dataset-two',catalog_version:{host:'two',global:1,project:1,project_sources:'other',source_epochs:{p:1}}},{project_id:'p'});
 assert.equal(await client.readConfiguration('query.configuration_index',{project_id:'p',kind:'role'},load),4);
 client.invalidateConfigurationQueries();
 assert.equal(await client.readConfiguration('query.configuration_index',{project_id:'p',kind:'role'},load),5);
 assert.equal(client.configurationMutation('configuration.change.apply'),true);
 assert.equal(client.configurationMutation('model_provider.revise'),true);
});
test('model TTL, failed-discovery backoff and account/runtime isolation cache metadata only',async()=>{
 client.invalidateModelCatalogs();let calls=0,fail=false,clock=100000;
 const original=Date.now;Date.now=()=>clock;
 const request=async()=>{calls++;if(fail)throw Error('offline');return {models:[{model:'private',unneeded:'drop'}],source:'fixture',unneeded:'drop'};};
 try {
   const first=await client.readModelCatalog(request,'openai',undefined,'account-a','runtime-a');
   assert.deepEqual(first.models,[{id:'private',displayName:undefined,context_window:undefined,max_output_tokens:undefined}]);
   assert.equal(first.unneeded,undefined);
   await client.readModelCatalog(request,'openai',undefined,'account-a','runtime-a');assert.equal(calls,1);
   clock+=300001;await client.readModelCatalog(request,'openai',undefined,'account-a','runtime-a');assert.equal(calls,2);
   await client.readModelCatalog(request,'openai',undefined,'account-b-runtime-b','runtime-b');assert.equal(calls,3);
   client.invalidateModelCatalogs();fail=true;
   await assert.rejects(client.readModelCatalog(request,'openai',undefined,'account-b-runtime-b','runtime-b'),/offline/);
   await assert.rejects(client.readModelCatalog(request,'openai',undefined,'account-b-runtime-b','runtime-b'),/offline/);assert.equal(calls,4);
   clock+=15001;fail=false;
   await client.readModelCatalog(request,'openai',undefined,'account-b-runtime-b','runtime-b');assert.equal(calls,5);
 } finally {Date.now=original;client.invalidateModelCatalogs();}
});
test('read-only permission browsing and runtime status polls do not invalidate catalogs',()=>{
 for(const method of ['permission.source.browse','permission.binding.preflight','configuration.change.preview','skill.export']) assert.equal(client.configurationMutation(method),false,method);
 assert.equal(client.modelMutation('runtime.preparation_status'),false);
 assert.equal(client.modelMutation('runtime.prepare'),true);
 assert.equal(client.configurationMutation('permission.source.select'),true);
});
test('bursts of committed versions retry reads within a bounded budget',async()=>{
 client.invalidateConfigurationQueries();let attempts=0;
 const result=await client.readConfiguration('query.configuration_index',{kind:'role'},async()=>{
  attempts++;if(attempts<4){client.invalidateConfigurationQueries();throw new DOMException('changed','AbortError');}return 'fresh';
 });
 assert.equal(result,'fresh');assert.equal(attempts,4);
});
test('project previews reuse exact inputs and invalidate on the next committed version',async()=>{
 client.invalidateConfigurationQueries();let calls=0;
 const load=async()=>({preview_hash:String(++calls)});
 const params={project_id:'p',standard_ref:'s:1',standard_overrides:[],view:'summary'};
 assert.deepEqual(await client.readConfiguration('query.project_standard_preview',params,load),{preview_hash:'1'});
 assert.deepEqual(await client.readConfiguration('query.project_standard_preview',params,load),{preview_hash:'1'});
 client.invalidateConfigurationQueries();
 assert.deepEqual(await client.readConfiguration('query.project_standard_preview',params,load),{preview_hash:'2'});
 assert.deepEqual(await client.readConfiguration('query.project_standard_preview',{...params,view:'full'},load),{preview_hash:'3'});
});
