// Real UI actions in independent BrowserContexts. No page.evaluate RPC load loop.
import {chromium} from '../../web/node_modules/playwright-core/index.mjs';
import fs from 'node:fs';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
const [fixtureFile, output, countText, secondsText]=process.argv.slice(2);
const fixture=JSON.parse(fs.readFileSync(fixtureFile,'utf8'));
const rpcContracts=JSON.parse(fs.readFileSync(new URL('../../crates/core/src/rpc_contracts.json',import.meta.url),'utf8'));
const readMethods=new Set(rpcContracts.entries.filter(entry=>entry.access==='read').map(entry=>entry.method));
const count=Number(countText), seconds=Number(secondsText);
fs.writeFileSync(output+'/browser-offered.json',JSON.stringify({clients:count,seconds,read_rps:count,write_rps:count/10,navigate_rps:count/20,planned_actions:Math.ceil(seconds*count)+Math.ceil(seconds*count/10)+Math.ceil(seconds*count/20),scope:'Initial setup and partial runs remain failures; planned work is never counted as completed'},null,2));
const browser=await chromium.launch({headless:true,channel:'chromium'});
const execAsync=promisify(execFile);
let monitoring=false,tearingDown=false,rpcSequence=0,rpcPhase='initialization';
const rssLimitMiB=Number(process.env.DEVDESK_BROWSER_RSS_LIMIT_MIB || '12288');
const resourceTimer=setInterval(async()=>{
 if(monitoring)return;monitoring=true;
 try{const {stdout}=await execAsync('ps',['-axo','pid=,ppid=,%cpu=,rss=,time=']);const processes=stdout.trim().split('\n').map(line=>line.trim().split(/\s+/));const descendants=new Set([process.pid]);let previous=0;while(previous!==descendants.size){previous=descendants.size;for(const row of processes)if(descendants.has(Number(row[1])))descendants.add(Number(row[0]));}const selected=processes.filter(row=>descendants.has(Number(row[0])));const rssMiB=selected.reduce((sum,row)=>sum+Number(row[3]),0)/1024;fs.appendFileSync(output+'/browser-resources.jsonl',JSON.stringify({time:Date.now(),processes:selected,rss_mib:rssMiB,rss_limit_mib:rssLimitMiB})+'\n');if(rssMiB>rssLimitMiB){recordAction({kind:'resource_abort',error:'Local browser generator exceeded its fixed RSS sum budget',rss_mib:rssMiB,rss_limit_mib:rssLimitMiB,done:Date.now()});process.kill(process.pid,'SIGTERM');}}catch(error){errors.push({monitor_error:String(error)});}finally{monitoring=false;}
},2000);
const actions=[],rpc=[],errors=[],pages=[],contexts=[],pendingResponses=[],eventLatencies=[];
const signalAborts=new Map();
const goto=async(page,url)=>{page.jointNavigationCounter=(page.jointNavigationCounter||0)+1;page.jointNavigating=true;return page.goto(url);};
const recordAction=row=>{actions.push(row);fs.appendFileSync(output+'/browser-actions.jsonl',JSON.stringify(row)+'\n');};
for(const signal of ['SIGINT','SIGTERM'])process.once(signal,()=>{tearingDown=true;fs.writeFileSync(output+'/browser-partial.json',JSON.stringify({signal,actions,rpc,errors},null,2));clearInterval(resourceTimer);void browser.close().finally(()=>{fs.writeFileSync(output+'/browser-rpc.jsonl',rpc.map(r=>JSON.stringify(r)+'\n').join(''));process.exit(1);});});
const stats=values=>{
 const ordered=[...values].sort((a,b)=>a-b);
 const percentile=p=>ordered[Math.ceil(ordered.length*p)-1]??null;
 return {count:ordered.length,p50_ms:percentile(.5),p95_ms:percentile(.95),
  p99_ms:percentile(.99),max_ms:ordered.at(-1)??null};
};
try {
 const probe=await browser.newContext();const response=await probe.request.get(fixture.origin);if(!response.ok())throw Error('Host unavailable');
 const roots=[];
 for(const root of fixture.roots){const v=await probe.request.post(fixture.origin+'/rpc',{data:{jsonrpc:'2.0',id:1,method:'query.workflow_instance',params:{project_id:root.project_id,id:root.id,view:'status'}}}).then(r=>r.json());if(v.error)throw Error(JSON.stringify(v.error));roots.push({...root,feature_id:v.result.runtime.owner_id});}
 await probe.close();
 const url=(root,detail)=>`${fixture.origin}/?tab=execution&detail=${detail}&project_id=${root.project_id}&feature_id=${detail==='context'?root.write_owner_id:root.feature_id}`;
 for(let at=0;at<count;at+=5) await Promise.all(Array.from({length:Math.min(5,count-at)},async(_,offset)=>{
  const index=at+offset, root=roots[index%roots.length];
  const context=await browser.newContext({viewport:{width:1280,height:950}});contexts.push(context);
  await context.addInitScript(()=>{window.__jointLongTasks=[];new PerformanceObserver(list=>{for(const e of list.getEntries())if(window.__jointLongTasks.length<1000)window.__jointLongTasks.push({start:e.startTime,ms:e.duration});}).observe({type:'longtask',buffered:true});});
  await context.exposeBinding('__jointRpcAbort',(_source,traceId,reason)=>{
   signalAborts.set(traceId,reason);
   fs.appendFileSync(output+'/browser-abort-signals.jsonl',
    JSON.stringify({client:index,trace_id:traceId,reason})+'\n');
  });
  await context.addInitScript(()=>{
   const nativeFetch=window.fetch;
   window.fetch=(input,init)=>{
    const traceId=init?.headers && new Headers(init.headers).get('x-devdesk-trace-id');
    const signal=init?.signal;
    if (traceId && signal) {
     signal.addEventListener('abort',()=>{
      void window.__jointRpcAbort(traceId,signal.reason?.name||'unknown').catch(()=>{});
     },{once:true});
    }
    return nativeFetch.call(window,input,init);
   };
  });
  const page=await context.newPage();pages[index]=page;page.setDefaultTimeout(10000);
  const network=await context.newCDPSession(page);
  await network.send('Network.enable');
  network.on('Network.eventSourceMessageReceived',({eventId,data})=>{
   try {
    const event=JSON.parse(data);
    if (!eventId || !Number.isFinite(event.time)) return;
    const ms=Date.now()-event.time;
    if (rpcPhase==='load') eventLatencies.push(ms);
    const observation={client:index,phase:rpcPhase,id:eventId,kind:event.kind,ms};
    fs.appendFileSync(output+'/browser-sse.jsonl',JSON.stringify(observation)+'\n');
   } catch {
    // Timing capture must not change browser event handling.
   }
  });
  page.on('framenavigated',frame=>{
   if (frame===page.mainFrame()) page.jointNavigationCounter=(page.jointNavigationCounter||0)+1;
  });
  page.on('pageerror',error=>errors.push({client:index,error:String(error)}));
  page.on('request',request=>{if(!request.url().endsWith('/rpc'))return;const call=request.postDataJSON();const row={request_id:++rpcSequence,trace_id:request.headers()['x-devdesk-trace-id'],phase:rpcPhase,client:index,method:call.method,project_id:call.params?.project_id,context_id:call.params?.context_id,owner_id:call.params?.owner_id,view:call.params?.view,navigation:page.jointNavigationCounter||0,start:Date.now()};rpc.push(row);request.jointRow=row;fs.appendFileSync(output+'/browser-rpc-events.jsonl',JSON.stringify({event:'sent',...row})+'\n');});
  page.on('requestfinished',request=>{if(!request.jointRow)return;pendingResponses.push((async()=>{const row=request.jointRow;row.ms=Date.now()-row.start;try{const response=await request.response();row.host_request_id=response.headers()['x-devdesk-request-id'];const body=await response.body();row.bytes=body.length;const value=JSON.parse(body);row.error=value.error;}catch(error){row.error=String(error);if(readMethods.has(row.method)&&((tearingDown&&/Target.*closed|Execution context was destroyed/.test(row.error))||((page.jointNavigating||row.navigation<(page.jointNavigationCounter||0))&&/No resource with given identifier found|Response body is not available/.test(row.error))))row.cancelled_by_client=tearingDown?'teardown':'ui_navigation';}finally{fs.appendFileSync(output+'/browser-rpc-events.jsonl',JSON.stringify({event:'finished',...row})+'\n');fs.appendFileSync(output+'/browser-rpc.jsonl',JSON.stringify(row)+'\n');}})());});
  page.on('requestfailed',request=>{if(request.jointRow){const row=request.jointRow;row.ms=Date.now()-row.start;row.error=request.failure();if(readMethods.has(row.method)&&row.error?.errorText==='net::ERR_ABORTED'&&(tearingDown||page.jointNavigating))row.cancelled_by_client=tearingDown?'teardown':'ui_navigation';fs.appendFileSync(output+'/browser-rpc-events.jsonl',JSON.stringify({event:'failed',...row})+'\n');fs.appendFileSync(output+'/browser-rpc.jsonl',JSON.stringify(row)+'\n');}});
  const start=Date.now();const role=count<3?'mixed':index<Math.ceil(count*.5)?'read':index<Math.min(count-1,Math.ceil(count*.8))?'write':'navigate';page.jointRole=role;page.jointRoot=root;page.jointSlot=index%roots.length;
  await goto(page,url(root,role==='write'||role==='mixed'?'context':'manager'));
  if(role==='write'||role==='mixed') {
   // The factory emits 25 explicit contexts per logical root. Bind a stable
   // resource identity; editing must not follow an ordinal that can move.
   page.jointContext=fixture.contexts[(index%roots.length)*25+Math.floor(index/roots.length)];
   if(!page.jointContext || page.jointContext.project_id!==root.project_id)throw Error('Missing independent browser write target');
   await page.getByText('共享工作背景 · 手工维护（不代表当轮已发送）',{exact:true}).click({noWaitAfter:true});await page.locator(`[data-context-id="${page.jointContext.id}"] summary`).click({noWaitAfter:true});
   await page.locator('section').filter({has:page.locator(`a.raw-json-link[href*="context_id=${page.jointContext.id}"]`)}).last().getByRole('button',{name:'编辑上下文',exact:true}).waitFor();
  }
  else await page.getByRole('button',{name:'刷新管理状态',exact:true}).first().waitFor();
  recordAction({client:index,kind:'initial',ms:Date.now()-start,done:Date.now()});fs.appendFileSync(output+'/browser-progress.jsonl',JSON.stringify({phase:'initial',client:index,ms:Date.now()-start,time:Date.now()})+'\n');
 }));
 const chains=Array.from({length:count},()=>Promise.resolve());
 const group=role=>pages.map((page,index)=>page.jointRole===role||page.jointRole==='mixed'?index:-1).filter(n=>n>=0);
 const groups={read:group('read'),write:group('write'),navigate:group('navigate')};
 if(new Set(groups.write.map(index=>pages[index].jointContext.id)).size!==groups.write.length)throw Error('Browser writers share a target unexpectedly');
 const rates={read:count,write:count/10,navigate:count/20};
 const scheduled=[];for(const [kind,rate]of Object.entries(rates))if(groups[kind].length)for(let n=0;n<seconds*rate;n++)scheduled.push({at:n/rate,kind,n});scheduled.sort((a,b)=>a.at-b.at);
 const began=Date.now();const futures=[];rpcPhase='load';
 fs.writeFileSync(output+'/browser-window.json',JSON.stringify({began,offered_end:began+seconds*1000},null,2));
 for(const task of scheduled){const delay=began+task.at*1000-Date.now();if(delay>0)await new Promise(r=>setTimeout(r,delay));const client=groups[task.kind][task.n%groups[task.kind].length];const page=pages[client];
  const work=chains[client].then(async()=>{const sent=Date.now();const row={client,kind:task.kind,sequence:task.n,scheduled:began+task.at*1000,sent,generator_queue_ms:sent-began-task.at*1000};try{
   if(row.generator_queue_ms>30000){row.sent=null;throw Error('Unsent: local browser generator queue exceeded 30 seconds');}
   if(task.kind==='navigate')page.jointNavigating=true;
   if(page.jointRole!=='write'&&page.jointRole!=='mixed'){
    const current=JSON.parse(fs.readFileSync(fixtureFile,'utf8')).roots[page.jointSlot];
    if(current.id!==page.jointRoot.id){page.jointNavigating=true;await goto(page,url(current,'manager'));page.jointRoot=current;row.renewed_root=current.id;}
   }
   if(page.jointRole==='mixed' && task.kind!=='write' && new URL(page.url()).searchParams.get('detail')!=='manager') await goto(page,url(page.jointRoot,'manager'));
   if(task.kind==='read'){
    const section=page.locator(`[data-instance-id="${page.jointRoot.id}"][data-manual-refresh]`).first();const previous=await section.getAttribute('data-manual-refresh');
    await section.getByRole('button',{name:'刷新管理状态',exact:true}).first().click({timeout:10000,noWaitAfter:true});
    await page.locator(`[data-instance-id="${page.jointRoot.id}"][data-manual-refresh]:not([data-manual-refresh="${previous}"])`).first().waitFor({timeout:10000});
   }
   else if(task.kind==='write'){
    if(page.jointRole==='mixed' && new URL(page.url()).searchParams.get('detail')!=='context'){await goto(page,url(page.jointRoot,'context'));await page.getByText('共享工作背景 · 手工维护（不代表当轮已发送）',{exact:true}).click({noWaitAfter:true});await page.locator(`[data-context-id="${page.jointContext.id}"] summary`).click({noWaitAfter:true});}
    row.target=page.jointContext.id;
    await page.locator('section').filter({has:page.locator(`a.raw-json-link[href*="context_id=${row.target}"]`)}).last().getByRole('button',{name:'编辑上下文',exact:true}).click({timeout:10000,noWaitAfter:true});
    const dialog=page.getByRole('dialog',{name:'更新上下文',exact:true});await dialog.getByRole('button',{name:'内容',exact:true}).click({noWaitAfter:true});await dialog.getByRole('textbox',{name:'内容',exact:true}).fill(`Browser client ${client} durable change ${task.n}`);
    const reply=page.waitForResponse(r=>r.url().endsWith('/rpc')&&r.request().postDataJSON()?.method==='context.update',{timeout:10000});void reply.catch(()=>{});
    await dialog.getByRole('button',{name:'确认',exact:true}).click({timeout:10000,noWaitAfter:true});const value=await(await reply).json();if(value.error)throw Error(JSON.stringify(value.error));await dialog.waitFor({state:'hidden',timeout:10000});
   }else{if(page.jointRole!=='mixed'&&Math.floor(task.n/groups.navigate.length)%3===2){page.jointSlot=(page.jointSlot+2)%roots.length;const next=JSON.parse(fs.readFileSync(fixtureFile,'utf8')).roots[page.jointSlot];const picker=page.getByLabel('当前项目',{exact:true});if(await picker.locator(`option[value="${next.project_id}"]`).count()){await picker.selectOption(next.project_id);row.project_route='selector';}else{row.project_route='explicit_url';}await goto(page,url(next,'manager'));page.jointRoot=next;row.project_switch=next.project_id;}const detail=Math.floor(task.n/groups.navigate.length)%2?'项目经理':'工作流程';await page.getByRole('tab',{name:detail,exact:true}).click({timeout:10000,noWaitAfter:true});if(detail==='项目经理')await page.getByRole('button',{name:'刷新管理状态',exact:true}).first().waitFor({timeout:10000});else await page.getByRole('button',{name:'刷新进度',exact:true}).first().waitFor({timeout:10000});}
  }catch(error){row.error=String(error);}finally{page.jointNavigating=false;row.done=Date.now();row.ms=row.done-sent;row.scheduled_ms=row.done-row.scheduled;recordAction(row);if(actions.length%100===0)fs.appendFileSync(output+'/browser-progress.jsonl',JSON.stringify({phase:'actions',completed:actions.length,failures:actions.filter(a=>a.error).length,time:Date.now()})+'\n');}});chains[client]=work.catch(()=>{});futures.push(work);
 }
 await Promise.all(futures);
 rpcPhase='verification';
 await Promise.allSettled(pendingResponses);
 const readbacks=[];
 for(const client of groups.write){
  const page=pages[client],target=page.jointContext,successful=actions.filter(a=>a.client===client&&a.kind==='write'&&!a.error);
  const expectedRevision=target.revision+successful.length;
  const response=await page.request.post(fixture.origin+'/rpc',{data:{jsonrpc:'2.0',id:'committed-readback-'+client,method:'query.workflow_contexts',params:{project_id:target.project_id,owner_id:page.jointRoot.write_owner_id,context_id:target.id}}});
  const value=await response.json();const actual=value.result?.context;
  const last=successful.at(-1);
  const valid=!value.error&&actual?.id===target.id&&actual.revision===expectedRevision&&(!last||actual.content===`Browser client ${client} durable change ${last.sequence}`);
  const row={client,target:target.id,successful_writes:successful.length,expected_revision:expectedRevision,actual_revision:actual?.revision,passed:valid,error:value.error};readbacks.push(row);fs.appendFileSync(output+'/browser-readbacks.jsonl',JSON.stringify(row)+'\n');
  if(!valid)errors.push({committed_readback:row});
 }
 const longTasks=[];for(let i=0;i<pages.length;i++){longTasks.push({client:i,visibility:await pages[i].evaluate(()=>document.visibilityState),tasks:await pages[i].evaluate(()=>window.__jointLongTasks)});if(i<4)await pages[i].screenshot({path:`${output}/client-${i}.png`});}
 tearingDown=true;clearInterval(resourceTimer);await browser.close();await Promise.allSettled(pendingResponses);
 for (const row of rpc) {
  if (readMethods.has(row.method) && row.error?.errorText==='net::ERR_ABORTED'
   && signalAborts.get(row.trace_id)==='AbortError') {
   row.cancelled_by_client??='confirmed_abort_signal';
  }
 }
 const summary={event_commit_field_to_receive:stats(eventLatencies),browser_contexts:contexts.length,clients:count,seconds,offered_rates:rates,generator_rss_limit_mib:rssLimitMiB,scheduled_actions:scheduled.length,actual_rpc:rpc.length,committed_readbacks:readbacks,errors,failures:actions.filter(a=>a.error),rpc_errors:rpc.filter(r=>r.error&&!r.cancelled_by_client),intentional_read_cancellations:rpc.filter(r=>r.cancelled_by_client),cancellation_scope:'Only query aborts observed during UI navigation, client teardown or on the exact request signal; mutation errors, timeouts, domain failures and unsent actions remain failures. Every request/event is retained.',formal_p6:false};
 for(const kind of ['initial','read','write','navigate']){const selected=actions.filter(a=>a.kind===kind);summary[kind]={...stats(selected.map(a=>a.ms)),scheduled_latency:stats(selected.map(a=>a.scheduled_ms).filter(v=>v!=null)),generator_queue:stats(selected.map(a=>a.generator_queue_ms).filter(v=>v!=null))};}
 summary.by_method=Object.fromEntries([...new Set(rpc.map(r=>r.method))].map(method=>[method,{...stats(rpc.filter(r=>r.method===method&&r.ms!=null).map(r=>r.ms)),bytes:rpc.filter(r=>r.method===method).reduce((n,r)=>n+(r.bytes||0),0)}]));
 summary.by_method_load=Object.fromEntries([...new Set(rpc.filter(r=>r.phase==='load').map(r=>r.method))].map(method=>[method,{...stats(rpc.filter(r=>r.phase==='load'&&r.method===method&&r.ms!=null&&!r.cancelled_by_client).map(r=>r.ms)),failed:rpc.filter(r=>r.phase==='load'&&r.method===method&&r.error&&!r.cancelled_by_client).length,cancelled:rpc.filter(r=>r.phase==='load'&&r.method===method&&r.cancelled_by_client).length}]));
 fs.writeFileSync(output+'/browser-actions.jsonl',actions.map(r=>JSON.stringify(r)+'\n').join(''));fs.writeFileSync(output+'/browser-rpc.jsonl',rpc.map(r=>JSON.stringify(r)+'\n').join(''));fs.writeFileSync(output+'/browser-long-tasks.json',JSON.stringify(longTasks));fs.writeFileSync(output+'/browser-summary.json',JSON.stringify(summary,null,2));
 console.log(JSON.stringify(summary,null,2));if(summary.failures.length||errors.length||summary.rpc_errors.length)process.exitCode=1;
}catch(error){fs.writeFileSync(output+'/browser-partial.json',JSON.stringify({error:String(error),actions,rpc,errors},null,2));throw error;}finally{clearInterval(resourceTimer);await browser.close();}
