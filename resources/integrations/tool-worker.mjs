// Trusted executor entrypoint. Service packages run only inside the caller's Docker sandbox.
import fs from 'node:fs';
import http from 'node:http';
import https from 'node:https';
import dns from 'node:dns/promises';
import net from 'node:net';
import { spawn } from 'node:child_process';
const request = JSON.parse(fs.readFileSync(process.argv[2] === '-' ? 0 : process.argv[2] || '/state/request.json', 'utf8'));
const { source, binding } = request;
const max = binding.max_result_bytes || 1048576;
const timeout = binding.timeout_ms || 30000;
let effect = 'not_applied';
const secrets = [request.secret, request.oauth?.client_secret, request.oauth?.code, request.oauth?.verifier].filter(x => typeof x === 'string' && x);
function clean(value) {
  if (typeof value === 'string') return secrets.reduce((s, secret) => s.split(secret).join('[redacted]'), value);
  if (Array.isArray(value)) return value.map(clean);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k,v]) => [k, /authorization|access_token|refresh_token|password|secret/i.test(k) ? '[redacted]' : clean(v)]));
  return value;
}
function fail(code, message) { const e = new Error(message); e.code = code; throw e; }
function privateAddress(ip) {
  const s = ip.toLowerCase();
  if (s.startsWith('::ffff:')) return privateAddress(s.slice(7));
  if (s.includes(':')) return s === '::1' || s === '::' || /^(fc|fd|fe8|fe9|fea|feb|ff)/.test(s);
  const a = s.split('.').map(Number);
  return a[0] === 0 || a[0] === 10 || a[0] === 127 || a[0] >= 224 || a[0] === 169 && a[1] === 254 || a[0] === 172 && a[1] >= 16 && a[1] <= 31 || a[0] === 192 && [0,168].includes(a[1]) || a[0] === 100 && a[1] >= 64 && a[1] <= 127;
}
async function transport(url, method, headers, body) {
  const u = new URL(url);
  if (!['http:','https:'].includes(u.protocol) || u.username || u.password || u.hash) fail('endpoint_denied','Invalid service URL');
  if (!binding.allowed_origins.includes(u.origin)) fail('endpoint_denied','Origin is outside the binding');
  const host = u.hostname.replace(/^\[|\]$/g,'');
  const addresses = net.isIP(host) ? [{address:host,family:net.isIP(host)}] : await dns.lookup(host,{all:true});
  if (!addresses.length || !binding.allow_private_network && addresses.some(a => privateAddress(a.address))) fail('endpoint_denied','Private or unknown address is not authorized');
  if (u.protocol === 'http:' && (!binding.allow_private_network || addresses.some(a => !privateAddress(a.address)))) fail('endpoint_denied','Plain HTTP is limited to explicitly allowed private services');
  const pin = addresses[0];
  return new Promise((resolve,reject) => {
    const client = u.protocol === 'https:' ? https : http;
    const req = client.request(u, { method, headers, timeout, lookup:(_h, opts, cb) => opts.all ? cb(null,[pin]) : cb(null,pin.address,pin.family) }, res => {
      let size=0; const chunks=[];
      res.on('data',chunk => { size+=chunk.length; if(size>max){req.destroy(Object.assign(new Error('Response exceeds configured limit'),{code:'result_too_large'}));return;} chunks.push(chunk); });
      res.on('error',reject);
      res.on('end',() => resolve({status:res.statusCode,headers:res.headers,body:Buffer.concat(chunks)}));
    });
    req.on('error',reject);req.on('timeout',()=>req.destroy(Object.assign(new Error('Service timeout'),{code:'timeout'})));
    effect = request.tool?.effect === 'read_only' || request.action === 'discover' ? 'not_applied' : 'unknown';
    req.end(body);
  });
}
function authHeaders() {
  const auth = binding.auth || {kind:'none'};
  if (auth.kind === 'none') return {};
  if (!request.secret) fail('credential_missing','Service credential is unavailable');
  return auth.kind === 'api_key' ? {[auth.header]:request.secret} : {Authorization:`Bearer ${request.secret}`};
}
function pointer(value, path) {
  if(path === '' || path == null) return value;
  return path.split('/').slice(1).reduce((v,k) => v?.[k.replace(/~1/g,'/').replace(/~0/g,'~')],value);
}
function sseFrames(buffer) {
  return buffer.toString('utf8').split(/\r?\n\r?\n/).map(frame=>frame.split(/\r?\n/).filter(l=>l.startsWith('data:')).map(l=>l.slice(5).trimStart()).join('\n')).filter(Boolean).map(data=>JSON.parse(data));
}
async function api() {
  const { tool, arguments:args } = request;
  const def = tool.implementation;
  let path = def.path;
  const headers = {Accept:'application/json, text/plain, application/octet-stream',...authHeaders()};
  const query = new URLSearchParams();
  for (const p of def.parameters || []) {
    const value=pointer(args,p.pointer);
    if(value === undefined){if(p.required)fail('argument_required',`Missing mapped parameter: ${p.name}`);continue;}
    if (p.location === 'path') {
      if (['.','..'].includes(String(value))) fail('argument_invalid','Dot path segments are forbidden');
      path = path.split(`{${p.name}}`).join(encodeURIComponent(String(value)));
    } else if(p.location === 'query') {
      for(const part of Array.isArray(value)?value:[value]) {if(part && typeof part === 'object')fail('unsupported_serialization','Object query values need an explicit supported encoding');query.append(p.name,part===null?'':String(part));}
    } else if(p.location === 'header') {
      if (Object.keys(headers).some(k=>k.toLowerCase()===p.name.toLowerCase()) || /^(host|authorization|cookie|connection|content-length|transfer-encoding|proxy-authorization)$/i.test(p.name)) fail('header_denied','Mapped header overrides protocol or authentication');
      headers[p.name]=String(value);
    }
  }
  if (/[{}]/.test(path)) fail('argument_required','Unresolved path parameter');
  const base=new URL(source.base_url);
  const url = new URL(source.base_url.replace(/\/$/,'')+path);
  if(url.origin!==base.origin || base.pathname!=='/' && !url.pathname.startsWith(base.pathname.replace(/\/$/,'')+'/'))fail('endpoint_denied','Path escapes configured API prefix');
  url.search=query.toString();
  let body;
  if(def.body_pointer != null){body=JSON.stringify(pointer(args,def.body_pointer));headers['Content-Type']='application/json';}
  if(def.idempotency_header)headers[def.idempotency_header]=request.invocation_id;
  const res=await transport(url,def.method,headers,body);
  if(res.status>=300 && res.status<400)fail('redirect_denied','Service redirects require a new explicit endpoint binding');
  const mime=String(res.headers['content-type']||'application/octet-stream').split(';')[0].trim();
  const text=res.body.toString('utf8');
  let value;
  if(mime==='application/json'||mime.endsWith('+json')){try{value=JSON.parse(text);}catch{fail('invalid_json','Service returned invalid JSON');}}
  const success=res.status>=200&&res.status<300;
  if(success && request.tool.effect!=='read_only')effect='applied';
  if (value!==undefined) return {success,content:[{type:'text',text:JSON.stringify(clean(value))}],structured_content:clean(value),remote_effect:effect,...(!success?{error:{code:'http_error',status:res.status}}:{})};
  if(mime.startsWith('text/'))return {success,content:[{type:'text',text:clean(text)}],remote_effect:effect,...(!success?{error:{code:'http_error',status:res.status}}:{})};
  return {success,content:[{type:'artifact',mime,base64:res.body.toString('base64')}],remote_effect:effect,...(!success?{error:{code:'http_error',status:res.status}}:{})};
}
async function mcp() {
  const modern=source.protocol_version==='2026-07-28';
  let counter=1,session,child;const waiting=new Map();let lines='';
  const callbacks=[];
  const accept = message => {
    if(message.method && message.id!=null){callbacks.push(message);return;}
    if(waiting.has(message.id)){const {resolve,reject,timer}=waiting.get(message.id);clearTimeout(timer);waiting.delete(message.id);message.error?reject(Object.assign(new Error(message.error.message||'MCP protocol error'),{code:'mcp_protocol',detail:message.error})):resolve(message.result);}
  };
  if(source.transport==='stdio') {
    if(!Array.isArray(source.command)||!source.command.length)fail('invalid_command','MCP command is missing');
    const env={PATH:process.env.PATH,HOME:'/tmp',TMPDIR:'/tmp'};
    if(request.secret)env[source.credential_env||'MCP_API_KEY']=request.secret;
    child=spawn(source.command[0],source.command.slice(1),{cwd:'/work',env,stdio:['pipe','pipe','pipe']});
    child.stderr.on('data',()=>{});
    child.stdout.on('data',chunk=>{lines+=chunk.toString();if(lines.length>max){child.kill();return;}let p;while((p=lines.indexOf('\n'))>=0){const line=lines.slice(0,p);lines=lines.slice(p+1);if(line.trim())try{accept(JSON.parse(line));}catch{child.kill();}}});
    child.on('error',e=>{for(const p of waiting.values()){clearTimeout(p.timer);p.reject(e);}waiting.clear();});
    child.on('exit',()=>{for(const p of waiting.values()){clearTimeout(p.timer);p.reject(Object.assign(new Error('MCP process exited'),{code:'mcp_disconnected'}));}waiting.clear();});
  }
  async function rpc(method,params={},notification=false) {
    const id=counter++;
    if(modern)params={...params,_meta:{...(params._meta||{}),'io.modelcontextprotocol/protocolVersion':'2026-07-28','io.modelcontextprotocol/clientInfo':{name:'devdesk',version:'1'},'io.modelcontextprotocol/clientCapabilities':{}}};
    const frame={jsonrpc:'2.0',...(notification?{}:{id}),method,params};
    if(child){if(notification){child.stdin.write(JSON.stringify(frame)+'\n');return;}return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{waiting.delete(id);reject(Object.assign(new Error('MCP timeout'),{code:'timeout'}));},timeout);waiting.set(id,{resolve,reject,timer});child.stdin.write(JSON.stringify(frame)+'\n');});}
    const headers={'Content-Type':'application/json',Accept:'application/json, text/event-stream','MCP-Protocol-Version':source.protocol_version,...authHeaders()};
    if(session&&!modern)headers['Mcp-Session-Id']=session;
    const res=await transport(source.url,'POST',headers,JSON.stringify(frame));
    if(res.status<200||res.status>=300)fail('mcp_http_error',`MCP HTTP status ${res.status}`);
    if(res.headers['mcp-session-id']&&!modern)session=String(res.headers['mcp-session-id']);
    if(notification)return;
    const frames=String(res.headers['content-type']).includes('text/event-stream')?sseFrames(res.body):[JSON.parse(res.body.toString())];
    for(const f of frames){if(f.id===id){if(f.error)throw Object.assign(new Error(f.error.message||'MCP protocol error'),{code:'mcp_protocol',detail:f.error});return f.result;}if(f.method&&f.id!=null)callbacks.push(f);}
    fail('mcp_invalid_response','No matching MCP response');
  }
  try {
    if(modern){const info=await rpc('server/discover');if(info?.protocolVersions&&!info.protocolVersions.includes(source.protocol_version))fail('mcp_version','Server does not support the pinned MCP version');}
    else {const init=await rpc('initialize',{protocolVersion:source.protocol_version,clientInfo:{name:'devdesk',version:'1'},capabilities:{}});if(init.protocolVersion!==source.protocol_version)fail('mcp_version','MCP negotiated a different pinned version');await rpc('notifications/initialized',{},true);}
    const tools=[];let cursor;const seen=new Set();
    do {const page=await rpc('tools/list',cursor?{cursor}:{});if(!Array.isArray(page.tools))fail('mcp_invalid_catalog','tools/list did not return tools');tools.push(...page.tools);if(tools.length>256)fail('mcp_limit','Too many remote tools');cursor=page.nextCursor;if(cursor&&seen.has(cursor))fail('mcp_invalid_catalog','Repeated MCP pagination cursor');if(cursor)seen.add(cursor);}while(cursor);
    if(new Set(tools.map(t=>t.name)).size!==tools.length)fail('mcp_invalid_catalog','Duplicate remote tool names');
    if(request.action==='discover')return {tools,protocol_version:source.protocol_version};
    const tool=tools.find(t=>t.name===request.tool.implementation.name);
    if(!tool)fail('mcp_tool_missing','Pinned remote tool has disappeared');
    const canonical=v=>Array.isArray(v)?v.map(canonical):v&&typeof v==='object'?Object.fromEntries(Object.keys(v).sort().map(k=>[k,canonical(v[k])])):v;
    const { createHash }=await import('node:crypto');
    const digest=createHash('sha256').update(JSON.stringify(canonical(tool))).digest('hex');
    if(digest!==request.tool.implementation.descriptor_hash)fail('mcp_descriptor_drift','Remote tool contract changed; discover and publish a new revision');
    effect=request.tool.effect==='read_only'?'not_applied':'unknown';
    const result=await rpc('tools/call',{name:tool.name,arguments:request.arguments});
    if(result.resultType && result.resultType!=='complete')fail('mcp_extension_unsupported','Tool requires an unsupported multi-round or asynchronous extension');
    if(callbacks.length)fail('mcp_extension_unsupported','Server requested an unsupported client interaction');
    const success=!result.isError;if(success&&request.tool.effect!=='read_only')effect='applied';
    return {success,content:clean(result.content||[]),...(Object.hasOwn(result,'structuredContent')?{structured_content:clean(result.structuredContent)}:{}),remote_effect:effect,...(!success?{error:{code:'mcp_tool_error'}}:{})};
  }finally{if(child){child.kill('SIGTERM');setTimeout(()=>child.kill('SIGKILL'),250).unref();}}
}
try {
  const result=source.kind==='http_api'?await api():source.kind==='mcp'?await mcp():fail('unsupported','Integration has no executable transport');
  process.stdout.write(JSON.stringify(request.action==='discover'?result:{...result,invocation_id:request.invocation_id,origin:'external'})+'\n');
}catch(error){
  process.stdout.write(JSON.stringify({invocation_id:request.invocation_id,success:false,content:[{type:'text',text:clean(String(error.message))}],remote_effect:effect,error:{code:error.code||'transport_error'},origin:'external'})+'\n');
}
