import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import crypto from 'node:crypto';
import {spawn} from 'node:child_process';
import {run,runtime,sha,atomic,edition} from './runtime.mjs';

const sleep = ms => new Promise(resolve=>setTimeout(resolve,ms));
export function identity(pid,root) {
  try {
    const row=run('/bin/ps',['-p',String(pid),'-o','uid=','-o','lstart=','-o','command=']).trim();
    if(Number(row.split(/\s+/)[0])!==process.getuid()) return null;
    // ps is discovery only. lsof independently binds the running text image.
    if(!/\bserve\b/.test(row)) return null;
    const files=run('/usr/sbin/lsof',['-a','-p',String(pid),'-d','txt','-Fn']).split('\n');
    const executable=files.filter(x=>x.startsWith('n')).map(x=>x.slice(1)).find(x=>{
      if(!x.startsWith(root+path.sep) || !x.endsWith('/bin/jcode')) return false;
      const relative=path.relative(root,x).split(path.sep);
      return relative.length===3 && relative[1]==='bin' && fs.existsSync(path.join(root,relative[0],'lite-manifest.json'));
    });
    if(!executable || fs.realpathSync(executable)!==executable) return null;
    return {pid, executable, stamp:row};
  } catch { return null; }
}
export function findServers(root) {
  const rows=run('/bin/ps',['-axo','pid=,uid=,command=']).split('\n');
  return rows.filter(row=>row.includes(root) && /\bserve\b/.test(row)).flatMap(row=>{
    const pid=Number(row.trim().split(/\s+/)[0]);
    const found=identity(pid,root);
    return found?[found]:[];
  });
}
function alive(pid) {
  try {return !run('/bin/ps',['-p',String(pid),'-o','stat=']).trim().startsWith('Z');}
  catch {return false;}
}
export async function stopServers(root,servers,report) {
  for(const server of servers) {
    const current=identity(server.pid,root);
    if(!current) {if(alive(server.pid)) throw Error('Cannot reverify live Lite server identity');continue;}
    if(current.stamp!==server.stamp || current.executable!==server.executable) throw Error('Server identity changed before stop');
    process.kill(server.pid,'SIGTERM');
    for(let i=0;i<100 && alive(server.pid);i++) await sleep(100);
    if(alive(server.pid)) throw Error(`Lite server ${server.pid} did not stop gracefully. No forced kill or shared lock deletion was attempted.`);
    report.push({step:'retire-server',status:'PASS',pid:server.pid});
  }
}
export function ping(socket) {
  return new Promise((resolve,reject)=>{
    const client=net.createConnection(socket); let data='';
    const timer=setTimeout(()=>client.destroy(Error('Server ping timeout')),1500);
    client.on('connect',()=>client.write('{"type":"ping","id":1}\n'));
    client.on('data',chunk=>{
      data+=chunk;
      if(data.length>65536) return client.destroy(Error('Oversized server response'));
      if(data.includes('\n')) {
        try { const msg=JSON.parse(data.split('\n')[0]); if(msg.type!=='pong'||msg.id!==1) throw Error('Unexpected server response'); clearTimeout(timer); client.destroy(); resolve(msg); }
        catch(error) { client.destroy(error); }
      }
    });
    client.on('error',error=>{clearTimeout(timer);reject(error);});
    client.on('close',()=>clearTimeout(timer));
  });
}
export async function startServer(root,version,report) {
  const rt=runtime(root), app=path.join(root,version);
  if(!fs.existsSync(path.join(app,'lifecycle/runtime.mjs'))) {
    const id=crypto.createHash('sha256').update(path.join(root,'home')).digest('hex').slice(0,16);
    rt.socket=`/tmp/jcl-${process.getuid()}-${id}-subscription-v1.sock`;
  }
  const existing=findServers(root);
  if(existing.length) throw Error('Unexpected Lite server appeared during update. Refusing parallel start.');
  const log=path.join(root,'runtime','server-start.log');
  const fd=fs.openSync(log,'a',0o600);
  const child=spawn(path.join(app,edition.launcher),['serve'],{
    detached:true,stdio:['ignore',fd,fd],cwd:process.env.HOME,
    env:{...process.env,JCODE_LITE_UPDATE_CHECK:'0',JCODE_RUNTIME_DIR:rt.runtime,JCODE_SOCKET:rt.socket},
  });
  fs.closeSync(fd); child.unref();
  let spawnError; child.on('error',error=>{spawnError=error;});
  for(let i=0;i<150;i++) {
    if(spawnError || child.exitCode!==null) throw Error(`New Lite server failed to start. See ${log}`);
    try {
      await ping(rt.socket);
      const actual=identity(child.pid,root);
      if(actual?.executable!==path.join(app,'bin/jcode')) throw Error('Responding server executable not yet verified');
      const client=JSON.parse(run(path.join(app,edition.launcher),['version','--json'],{env:{...process.env,JCODE_LITE_UPDATE_CHECK:'0'}}));
      const item={step:'server',status:'PASS',pid:child.pid,version:client.version,binary_sha256:sha(actual.executable),socket:rt.socket,runtime:rt.runtime};
      atomic(path.join(root,'runtime','server-identity.json'),JSON.stringify(item,null,2)+'\n');
      report.push(item); return child.pid;
    } catch { await sleep(200); }
  }
  const owned=identity(child.pid,root);
  if(owned) await stopServers(root,[owned],report);
  throw Error(`New Lite server did not become healthy. See ${log}`);
}
