// Real HTTP side-effect, timeout, file and redaction fixture. No model calls.
import http from 'node:http';
let writes=0;
const server=http.createServer(async(req,res)=>{
 for await(const _ of req) {}
 const url=new URL(req.url,'http://local');
 if(req.method==='POST'){writes++;process.stdout.write(JSON.stringify({writes})+'\n');}
 if(url.pathname==='/redirect'){res.writeHead(302,{Location:'/write'}).end();return;}
 if(url.pathname==='/binary'){res.writeHead(200,{'Content-Type':'application/octet-stream'}).end(Buffer.from([0,1,2,3,255]));return;}
 if(url.pathname==='/slow')await new Promise(resolve=>setTimeout(resolve,1500));
 res.writeHead(200,{'Content-Type':'application/json'}).end(JSON.stringify({writes,echo:url.searchParams.get('secret')}));
});
server.listen(0,'0.0.0.0',()=>process.stdout.write(JSON.stringify({port:server.address().port})+'\n'));
