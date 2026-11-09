// Actual local MCP protocol fixture; no model/provider requests.
import http from 'node:http';
import readline from 'node:readline';
const tools = [
  { name:'sum', description:'Add two integers', inputSchema:{type:'object',properties:{a:{type:'integer'},b:{type:'integer'}},required:['a','b'],additionalProperties:false}, outputSchema:{type:'object',properties:{sum:{type:'integer'}},required:['sum'],additionalProperties:false} },
  { name:'other', description:'Unselected action', inputSchema:{type:'object',additionalProperties:false} },
];
function answer(frame,headers={}) {
  if(frame.id==null)return null;
  const modern=frame.params?._meta?.['io.modelcontextprotocol/protocolVersion']==='2026-07-28';
  if(headers['mcp-protocol-version']==='2026-07-28'&&!modern)return {jsonrpc:'2.0',id:frame.id,error:{code:-32602,message:'Modern requests require protocol metadata'}};
  let result;
  switch(frame.method){
    case 'initialize': result={protocolVersion:'2025-11-25',capabilities:{tools:{}},serverInfo:{name:'local-fixture',version:'1'}};break;
    case 'server/discover': result={protocolVersions:['2026-07-28'],capabilities:{tools:{}}};break;
    case 'tools/list': result=frame.params?.cursor?{tools:[tools[1]]}:{tools:[tools[0]],nextCursor:'second'};break;
    case 'tools/call': {
      if(frame.params.name!=='sum')return {jsonrpc:'2.0',id:frame.id,error:{code:-32602,message:'Unselected fixture tool'}};
      const sum=frame.params.arguments.a+frame.params.arguments.b;
      result={...(modern?{resultType:'complete'}:{}),content:[{type:'text',text:`sum=${sum}`}],structuredContent:{sum}};break;
    }
    default:return {jsonrpc:'2.0',id:frame.id,error:{code:-32601,message:'Unsupported fixture method'}};
  }
  return {jsonrpc:'2.0',id:frame.id,result};
}
if(process.argv.includes('--http')) {
  const server=http.createServer(async(req,res)=>{
    const chunks=[];for await(const part of req)chunks.push(part);
    let frame;try{frame=JSON.parse(Buffer.concat(chunks).toString());}catch{res.writeHead(400).end();return;}
    if(frame.method!=='initialize' && req.headers['mcp-protocol-version']==='2025-11-25' && req.headers['mcp-session-id']!=='fixture-session'){res.writeHead(400).end();return;}
    const result=answer(frame,req.headers);
    if(!result){res.writeHead(202).end();return;}
    const headers={'Content-Type':'application/json'};
    if(frame.method==='initialize')headers['Mcp-Session-Id']='fixture-session';
    if(frame.method==='tools/call'){headers['Content-Type']='text/event-stream';res.writeHead(200,headers).end(`data: ${JSON.stringify(result)}\n\n`);return;}
    res.writeHead(200,headers).end(JSON.stringify(result));
  });
  server.listen(0,'0.0.0.0',()=>process.stdout.write(JSON.stringify({port:server.address().port})+'\n'));
}else {
  readline.createInterface({input:process.stdin}).on('line',line=>{const result=answer(JSON.parse(line));if(result)process.stdout.write(JSON.stringify(result)+'\n');});
}
