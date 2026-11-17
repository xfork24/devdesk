import { chromium } from '../../web/node_modules/playwright-core/index.mjs';
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
const [origin, output, count, serverPid, mode = "warm", selectedProject] = process.argv.slice(2);
const samples = Number(count);
const browser = await chromium.launch({ channel: 'chromium', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const cdp=await page.context().newCDPSession(page);
await cdp.send('Performance.enable');
await page.addInitScript(()=>{
  window.__configurationLongTasks=[];
  new PerformanceObserver(list=>{for(const item of list.getEntries()) if(window.__configurationLongTasks.length<500) window.__configurationLongTasks.push({start_ms:item.startTime,ms:item.duration});}).observe({type:'longtask',buffered:true});
});
const trace = [], navigation = [], errors = [], consoleErrors=[];
let phase = { sample: -1, page: 'cold' }, start = Date.now();
const pending = new Set();
const polls = new Set(['runtime.preparation_status', 'query.session', 'query.agent_experience']);
await page.route('**/rpc', async route => {
  const call = route.request().postDataJSON();
  if (call.method === 'query.model_catalog_identity') {
    await route.fulfill({json:{jsonrpc:'2.0',id:call.id,result:{identity:'performance-fixture',available:true}}});
  } else if (['codex.auth.models', 'models.list'].includes(call.method)) {
    await route.fulfill({ json: { jsonrpc: '2.0', id: call.id, result: { models: [], source: 'performance_fixture' } } });
  } else await route.continue();
});
page.on('pageerror', e => errors.push(String(e)));
page.on('console',message=>{if(message.type()==='error')consoleErrors.push(message.text().slice(0,500));});
page.on('request', r => {
  if (!r.url().endsWith('/rpc')) return;
  const data = r.postDataJSON();
  const row = { ...phase, method: data.method, params: data.params, start_ms: Date.now() - start };
  trace.push(row); r.profileRow = row;
  if (!polls.has(data.method)) pending.add(r);
});
page.on('requestfinished', async r => {
  if (!r.profileRow) return;
  const row = r.profileRow;
  try {
    const response = await r.response(), bytes = await response.body();
    row.end_ms = Date.now() - start; row.bytes = bytes.length;
    const value = JSON.parse(bytes);
    if (value.error) row.error = value.error;
  } catch (e) { row.failure = String(e); }
  finally { pending.delete(r); }
});
page.on('requestfailed', r => { if (r.profileRow) r.profileRow.failure = r.failure(); pending.delete(r); });
async function settle() {
  let quiet = Date.now();
  const end = Date.now() + 60000;
  while (Date.now() < end) {
    await new Promise(resolve => setTimeout(resolve, 50));
    if (pending.size) quiet = Date.now();
    else if (Date.now() - quiet > 350) { await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))); return; }
  }
  throw Error('configuration requests did not settle: ' + JSON.stringify(phase));
}
function save() { fs.writeFileSync(output + '/browser.json', JSON.stringify({ trace, navigation, errors, consoleErrors, realtime_models: 'explicit fixture; excluded', scheduler: false, mode }, null, 2)); }
try {
  await page.goto(origin + '/?tab=discussion'+(selectedProject?'&project_id='+encodeURIComponent(selectedProject):'')); await settle();
  const names = ['项目管理', '流程与规范', '角色与Agent', '基础组件', '角色讨论'];
  for (let sample = 0; sample < samples; sample++) for (const name of names) {
    phase = { sample, page: name }; start = Date.now();
    if (mode === 'cold') await page.goto(origin+'/?tab='+({'项目管理':'projects','流程与规范':'workflows','角色与Agent':'actors','基础组件':'components','角色讨论':'discussion'}[name])+(selectedProject?'&project_id='+encodeURIComponent(selectedProject):''));
    else await page.getByRole('button', { name: name === '角色与Agent' ? /^角色与(?:Agent|代理)$/ : name, exact: true }).click();
    if (name === '项目管理') await page.waitForFunction(()=>[...document.querySelectorAll('[aria-label="项目配置"] button')].some(b=>b.textContent.includes('保存项目配置') && !b.disabled));
    else if (name === '角色讨论') await page.locator('select[aria-label="讨论角色"] option[value]:not([value=""])').first().waitFor({state:'attached'});
    else await page.getByRole('region',{name: name === '流程与规范' ? '工作流程管理' : name === '角色与Agent' ? '角色管理' : 'Model 管理', exact:true}).locator('.configuration-list button strong').first().waitFor();
    await page.evaluate(()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r))));
    const ui_ms=Date.now()-start;
    await settle();
    const rows = trace.filter(r => r.sample === sample && r.page === name && !polls.has(r.method) && !['codex.auth.models','models.list'].includes(r.method));
    const finite = rows.filter(r => Number.isFinite(r.end_ms));
    if (rows.some(r => r.error || r.failure)) throw Error(JSON.stringify(rows.filter(r => r.error || r.failure)));
    navigation.push({ sample, page: name, ui_ms, data_ms: Math.max(0, ...finite.map(r => r.end_ms)), requests: rows.length, bytes: finite.reduce((n,r) => n+r.bytes,0), settled_ms: Date.now()-start });
    save();
  }
  await page.screenshot({ path: output + '/page.png', fullPage: true });
  const metrics=(await cdp.send('Performance.getMetrics')).metrics.filter(item=>['TaskDuration','ScriptDuration','LayoutDuration','RecalcStyleDuration','JSHeapUsedSize'].includes(item.name));
  fs.writeFileSync(output+'/main-thread.json',JSON.stringify({metrics,longtasks:await page.evaluate(()=>window.__configurationLongTasks)},null,2));
  function cpu() {
    if (process.platform !== 'darwin') return null;
    const value = execFileSync('ps',['-p',serverPid,'-o','time='],{encoding:'utf8'}).trim().split(':');
    return Number(value[0])*60+Number(value[1]);
  }
  // A Role page has no session poll. Keep only its event subscription and preparation status.
  await page.getByRole('button',{name:/^角色与(?:Agent|代理)$/,exact:true}).click(); await settle();
  const before=cpu();await new Promise(r=>setTimeout(r,5000));const active=cpu();
  await page.close();const closed=cpu();await new Promise(r=>setTimeout(r,5000));const after=cpu();
  fs.writeFileSync(output+'/idle-cpu.json',JSON.stringify({active_cpu_seconds:before===null?null:active-before,closed_cpu_seconds:closed===null?null:after-closed,seconds:5},null,2));
} finally { save(); await browser.close(); }
