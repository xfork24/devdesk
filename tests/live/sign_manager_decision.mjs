// Explicit test-operator Human signature. It cannot dispatch, accept or retry.
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import assert from 'node:assert/strict';
const require = createRequire(new URL('../../web/package.json', import.meta.url));
const { chromium, expect } = require('@playwright/test');
const [file, opinion] = process.argv.slice(2);
assert(opinion?.trim(), 'A reviewed decision needs an actual Human opinion');
const {request, manager_url: url} = JSON.parse(readFileSync(file,'utf8'));
const packet = request.data.request.interaction;
// The caller must read and archive the exact business materials first.
for (const material of packet.materials) assert(readFileSync(join(dirname(file),'reviewed-'+material.output_id+'.json'),'utf8'));
const browser=await chromium.launch({headless:true,channel:'chromium'});
const controls=[];
try {
 const context=await browser.newContext({viewport:{width:1440,height:1100}});
 const page=await context.newPage();
 page.on('request',r=>{
  if (!r.url().endsWith('/rpc')) return;
  const call=r.postDataJSON();
  if (!call.method.startsWith('query.') && call.method !== 'runtime.preparation_status') controls.push(call);
 });
 await page.goto(url);
 const card=page.getByRole('region',{name:'项目经理协作',exact:true}).getByRole('article',{name:packet.title,exact:true}).first();
 await expect(card).toBeVisible();
 await card.getByLabel('意见或修改要求').fill(opinion);
 await card.screenshot({path:join(dirname(file),request.id+'-before-signature.png')});
 const result=page.waitForResponse(r=>r.url().endsWith('/rpc') && r.request().postDataJSON()?.method==='workflow.decision.resolve');
 const approved=!process.argv.includes('--reject');
 await card.getByRole('button',{name:approved?'确认：'+packet.title:'退回并反馈意见',exact:true}).click();
 const response=await (await result).json();
 assert.equal(response.error,undefined,JSON.stringify(response.error));
 assert.deepEqual(controls.map(c=>c.method),['workflow.decision.resolve']);
 assert.equal(controls[0].params.target,request.id);
 writeFileSync(join(dirname(file),request.id+'-human-signature.json'),JSON.stringify({request_id:request.id,approved,opinion,controls,response},null,2)+'\n');
 console.log('SIGNED Human decision only',request.id);
} finally {await browser.close();}
