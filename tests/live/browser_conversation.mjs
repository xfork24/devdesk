// Real browser -> DevDesk RPC -> managed DSH -> structured proposal -> formal Requirement.
import { chromium } from '../../web/node_modules/playwright/index.mjs';
import fs from 'node:fs';
const reports=process.env.DEVDESK_LIVE_REPORTS||'/tmp/devdesk-live';const url=process.env.DEVDESK_LIVE_URL||'http://127.0.0.1:51751';
const source=JSON.parse(fs.readFileSync(reports+'/R3-dsh-application.json','utf8'));
const output=reports+'/R5-browser-proposal.json';
const report={passed:false,source:'real Chromium + DevDesk host + managed DSH',project_id:source.project_id,session_id:source.session_id};
const browser=await chromium.launch({headless:true});const page=await browser.newPage();
try{
 await page.goto(url+'/');await page.getByLabel('当前项目').selectOption(source.project_id);
 await page.getByRole('button',{name:'角色讨论',exact:true}).click();
 await page.getByLabel('讨论会话').selectOption(source.session_id);
 await page.getByRole('log',{name:'讨论记录'}).waitFor();
 const title='Acceptance calculator '+Date.now();
 await page.getByLabel('讨论输入').fill('Do not use tools. Return ONLY this exact ProposalContent JSON, without code fences: '+JSON.stringify({kind:'requirement',title,body:'Provide an integer addition function and a multiplication function with separate tests.',acceptance:[{key:'AC-ADD',description:'add(2,3) returns 5'},{key:'AC-MUL',description:'multiply(2,3) returns 6'}]}));
 await page.getByRole('button',{name:'发送',exact:true}).click();
 await page.locator('.conversation-message.assistant').filter({hasText:title}).waitFor({timeout:240000});
 await page.getByRole('button',{name:'发送',exact:true}).waitFor();
 await page.waitForFunction(()=>!document.querySelector('form button.primary')?.disabled,{timeout:30000});
 await page.getByText('结构化提案与采纳',{exact:true}).click();
 await page.getByRole('button',{name:'从完成的回答提取',exact:true}).click();
 const card=page.locator('details article').filter({has:page.getByRole('heading',{name:'requirement · draft',exact:true})});await card.waitFor();
 await card.getByRole('button',{name:'采纳',exact:true}).click();
 await page.getByRole('heading',{name:'requirement · accepted',exact:true}).waitFor();
 const result=await page.request.post(url+'/rpc',{data:{jsonrpc:'2.0',id:1,method:'query.workspace',params:{project_id:source.project_id}}});const view=(await result.json()).result;
 const doc=view.project_documents.find(d=>d.title===title);if(!doc||doc.acceptance.length!==2)throw new Error('formal_requirement_missing');
 Object.assign(report,{passed:true,requirement_id:doc.id,acceptance_keys:doc.acceptance.map(c=>c.key),no_fabricated_feature:view.features.length===0});
 await page.screenshot({path:reports+'/R5-browser-proposal.png',fullPage:true});
}catch(e){report.failure_type=e.constructor.name;report.failure_code=e.message.slice(0,240)}finally{await browser.close();fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));}
process.exitCode=report.passed?0:1;
