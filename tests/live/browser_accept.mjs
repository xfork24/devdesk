// Actual UI acceptance of an already independently reviewed native mainline candidate.
import {chromium} from '../../web/node_modules/playwright/index.mjs';
import fs from 'node:fs';
const args=Object.fromEntries(Array.from({length:(process.argv.length-2)/2},(_,i)=>[process.argv[2+i*2].replace(/^--/,''),process.argv[3+i*2]]));
const url=args.url||'http://127.0.0.1:51751', output=args.output;
const report={passed:false,project:args.project,feature:args.feature,merge:args.merge,candidate:args.candidate,source:'real Chromium UI -> Rust merge.apply -> Git CAS -> exact remote lease -> GitHub merged'};
const browser=await chromium.launch({headless:true});const page=await browser.newPage({viewport:{width:1450,height:1100}});const errors=[];page.on('pageerror',e=>errors.push(String(e)));
async function detail(){const r=await page.request.post(url+'/rpc',{data:{jsonrpc:'2.0',id:1,method:'query.workspace',params:{project_id:args.project,feature_id:args.feature,limit:100}}});const v=await r.json();if(v.error)throw new Error(v.error.data.code);return v.result.detail;}
try{
 await page.goto(url);await page.getByLabel('当前项目').selectOption(args.project);const state=await detail();
 await page.getByRole('button',{name:'工作总览',exact:true}).click();
 await page.locator('.feature-row').filter({hasText:state.feature.title}).click();
 await page.getByRole('button',{name:'执行与交付',exact:true}).click();
 const card=page.locator('article.delivery').filter({has:page.getByRole('heading',{name:'Feature → mainline',exact:true})}).filter({hasText:args.candidate});await card.waitFor({timeout:30000});
 const apply=card.getByRole('button',{name:'接纳候选并同步 GitHub',exact:true});await apply.waitFor({timeout:30000});
 page.on('request',request=>{if(request.url().endsWith('/rpc')){const body=request.postDataJSON();if(body?.method==='merge.apply'&&body.params.target===args.merge)report.command_id=body.params.command_id;}});
 await apply.click();
 await card.locator('.status-completed').waitFor({timeout:240000});const final=await detail();const merge=final.merges.find(m=>m.id===args.merge);
 if(!report.command_id||merge.status!=='completed'||final.feature.status!=='completed'||merge.committed_revision!==args.candidate||merge.change_request?.provider!=='github')throw new Error('final_remote_delivery_not_verified');
 const link=card.getByRole('link',{name:/GitHub PR/});report.pull_request=await link.getAttribute('href');report.passed=true;report.feature_status=final.feature.status;report.console_errors=errors;if(errors.length)throw new Error('browser_console_error');
 await page.screenshot({path:output.replace(/\.json$/,'.png'),fullPage:true});
}catch(e){report.passed=false;report.failure_type=e.constructor.name;report.failure=e.message.slice(0,500);}finally{await browser.close();fs.writeFileSync(output,JSON.stringify(report,null,2)+'\n');console.log(JSON.stringify(report,null,2));}
process.exitCode=report.passed?0:1;
