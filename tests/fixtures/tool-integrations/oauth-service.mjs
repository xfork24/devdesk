// A real local OAuth token endpoint for Docker protocol tests; no model calls.
import http from 'node:http';
const server=http.createServer(async(req,res)=>{
  let raw='';for await(const chunk of req)raw+=chunk;
  const form=new URLSearchParams(raw);
  res.setHeader('Content-Type','application/json');
  if(req.url==='/token' && form.get('grant_type')==='authorization_code' && form.get('code_verifier')?.length>=43){res.end(JSON.stringify({access_token:'first-token',refresh_token:'refresh-token',token_type:'Bearer',scope:'read',expires_in:0}));}
  else if(req.url==='/token' && form.get('grant_type')==='refresh_token' && form.get('refresh_token')==='refresh-token'){res.end(JSON.stringify({access_token:'renewed-token',scope:'read',expires_in:3600}));}
  else if(req.url==='/revoke'&&form.get('token')==='refresh-token'){res.end('{}');}
  else {res.writeHead(400).end(JSON.stringify({error:'invalid_grant'}));}
});
server.listen(0,'0.0.0.0',()=>process.stdout.write(JSON.stringify({port:server.address().port})+'\n'));
