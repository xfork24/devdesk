import {chromium} from '/home/hengliu/workspace/devdesk/web/node_modules/playwright/index.mjs';
import fs from 'node:fs';
const ctx=JSON.parse(fs.readFileSync('/tmp/devdesk-ubuntu-acceptance-context.json','utf8'));
const full=JSON.parse(fs.readFileSync('/tmp/devdesk-ubuntu-full-context.json','utf8'));
const source=JSON.parse(fs.readFileSync(full.reports+'/R7-merges.json','utf8'));
const browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:1450,height:1100}});const errors=[];page.on('pageerror',e=>errors.push(String(e)));
let report={passed:false,source:'actual extracted Linux package Chromium after restart, read-only completed delivery inspection'};
try{
 await page.goto(ctx.url);await page.getByLabel('当前项目').selectOption(source.project_id);await page.getByRole('button',{name:'工作总览',exact:true}).click();
 const response=await page.request.post(ctx.url+'/rpc',{data:{jsonrpc:'2.0',id:1,method:'query.workspace',params:{project_id:source.project_id,feature_id:source.feature_id,limit:100}}});const value=await response.json();if(value.error)throw Error(value.error.data.code);const detail=value.result.detail;
 if(detail.feature.status!=='completed')throw Error('feature_not_completed');
 await page.locator('.feature-row').filter({hasText:detail.feature.title}).click();await page.getByRole('button',{name:'执行与交付',exact:true}).click();
 const card=page.locator('article.delivery').filter({has:page.getByRole('heading',{name:'Feature → mainline',exact:true})}).filter({hasText:source.merges.mainline.candidate});await card.locator('.status-completed').waitFor();
 if(await card.getByRole('link',{name:/GitHub PR/}).getAttribute('href')!==source.merges.mainline.pull_request)throw Error('incorrect_PR');if(errors.length)throw Error('page_errors');
 await page.screenshot({path:ctx.reports+'/final-package-browser.png',fullPage:true});
 report={...report,passed:true,project:source.project_id,feature:source.feature_id,candidate:source.merges.mainline.candidate,feature_status:detail.feature.status,pull_request:source.merges.mainline.pull_request,console_errors:errors};
}finally{await browser.close();fs.writeFileSync(ctx.reports+'/final-package-browser.json',JSON.stringify(report,null,2)+'\n');}if(!report.passed)process.exitCode=1;console.log(JSON.stringify(report));
