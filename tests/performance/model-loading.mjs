import {chromium} from '../../web/node_modules/playwright-core/index.mjs';
import fs from 'node:fs';
const [origin,output,count]=process.argv.slice(2);
const browser=await chromium.launch({channel:'chromium',headless:true});
const rows=[];
try {
 for(const condition of ['slow','failure']) for(let sample=0;sample<Number(count);sample++) {
  const context=await browser.newContext({viewport:{width:1440,height:1000}});
  const page=await context.newPage();let modelRequests=0;
  const errors=[];page.on('pageerror',e=>errors.push(String(e)));
  await page.route('**/rpc',async route=>{
   const call=route.request().postDataJSON();
   if(call.method==='query.model_catalog_identity') return route.fulfill({json:{jsonrpc:'2.0',id:call.id,result:{identity:'performance-model-account',available:true}}});
   if(!['codex.auth.models','models.list'].includes(call.method)) return route.continue();
   modelRequests++;
   await new Promise(r=>setTimeout(r,1200));
   try {await route.fulfill({json:{jsonrpc:'2.0',id:call.id,...(condition==='failure'?{error:{message:'performance offline fixture'}}:{result:{models:[{model:'gpt-6.1-sol'}]}})}});}catch{/* context may close after its independent fields are ready */}
  });
  const start=Date.now();
  await page.goto(origin+'/?tab=components&config_tab=model&config_object=builtin-model-gpt-6-1-sol-high:1');
  await page.waitForFunction(()=>document.querySelector('select[aria-label="模型标识"]')?.value==='gpt-6.1-sol');
  const saved_ms=Date.now()-start;
  await page.waitForFunction(()=>{const select=document.querySelector('select[aria-label="模型标识"]');return select?.value==='gpt-6.1-sol' && !select.disabled;});
  await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
  const editable_ms=Date.now()-start;
  rows.push({condition,sample,saved_ms,editable_ms,modelRequests,errors,delay_ms:1200});
  fs.writeFileSync(output+'/model-conditions.json',JSON.stringify(rows,null,2));
  if(errors.length)throw Error(errors.join('\n'));
  await context.close();
 }
} finally {await browser.close();}
