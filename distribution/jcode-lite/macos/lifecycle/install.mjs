import fs from 'node:fs';
import path from 'node:path';
import {installation,privateDir,readState,versionPattern,copyPackage,packageFiles,run,sha,atomic,edition} from './runtime.mjs';
import {Journal,migrateHome,migrateAliases} from './migrate.mjs';
import {findServers,stopServers,startServer} from './server.mjs';

process.umask(0o077);
const source=fs.realpathSync(process.argv[2]);
const root=privateDir(installation());
const state=readState(root), runId=`${Date.now()}-${process.pid}`;
const work=privateDir(path.join(root,'updates',runId));
const lock=path.join(root,'update.lock');
const report=[];
const journal=new Journal(path.join(work,'backup'));
let locked=false, activated=false, stopped=false, target, displaced, stage, oldServers=[];
try {
  try {fs.mkdirSync(lock,{mode:0o700});locked=true;atomic(path.join(lock,'owner.json'),JSON.stringify({pid:process.pid,work}));}
  catch {throw Error(`Another update or interrupted transaction owns ${lock}. Do not delete it while its process is running.`);}
  if(process.platform!=='darwin'||process.arch!=='arm64') throw Error('This package requires Apple Silicon macOS');
  const release=JSON.parse(fs.readFileSync(path.join(source,'release.json')));
  if(release.product!==edition.product) throw Error('Package edition does not match installer');
  if(!versionPattern.test(release.version)) throw Error('Invalid package version');
  const names=packageFiles(source);
  const bytes=names.reduce((n,file)=>n+fs.statSync(path.join(source,file)).size,0);
  const disk=fs.statfsSync(root);
  if(disk.bavail*disk.bsize<bytes+256*1024*1024) throw Error('Not enough free disk for private staging and rollback');
  const version=JSON.parse(run(path.join(source,'bin/jcode'),['--no-update','version','--json'],{env:{...process.env,JCODE_HOME:path.join(work,'probe-home'),JCODE_NO_TELEMETRY:'1'}}));
  if(!version.version.startsWith(`v${release.jcode_version} (`)) throw Error('Client binary differs from release metadata');
  const cli=path.join(process.env.HOME,'.local/bin',edition.cli);
  if(fs.existsSync(cli) && (fs.lstatSync(cli).isSymbolicLink() || !fs.readFileSync(cli,'utf8').includes('LeGrin/'+edition.rootName))) throw Error('Existing jcodel is not a proven Lite launcher. It was not overwritten.');
  stage=path.join(root,`.staging-${runId}`);copyPackage(source,stage);
  for(const file of ['bin/jcode','runtime/node/bin/node']) run('/usr/bin/codesign',['--verify','--strict',path.join(stage,file)]);
  report.push({step:'preflight',status:'PASS',package:release.version,client:version.version,binary_sha256:sha(path.join(stage,'bin/jcode'))});
  console.log(`Verified Lite ${release.version}. Retiring only identified servers from this Lite installation...`);
  oldServers=findServers(root);
  stopped=oldServers.length>0; await stopServers(root,oldServers,report);
  target=path.join(root,release.version);
  if(fs.existsSync(target)) {displaced=path.join(work,'previous-package');fs.renameSync(target,displaced);}
  fs.renameSync(stage,target);stage=null;activated=true;
  const prior=state.current?(state.current===release.version?displaced:path.join(root,state.current)):null;
  migrateHome(root,target,prior,journal,report);
  const mcp=path.join(root,'home/mcp.json');
  journal.write(mcp,fs.existsSync(mcp)?fs.readFileSync(mcp):'{}\n');
  run(path.join(target,'runtime/node/bin/node'),[path.join(target,'preset/mcp/bin/merge-mcp.mjs'),'--manifest',path.join(target,'preset/mcp/manifest.json')],{env:{...process.env,JCODE_HOME:path.join(root,'home')}});
  journal.write(path.join(root,'state.json'),JSON.stringify({current:release.version,previous:state.current===release.version?state.previous:state.current})+'\n');
  journal.write(cli,fs.readFileSync(path.join(target,edition.launcher)),0o755);
  migrateAliases(root,journal,report);
  const profile=path.join(process.env.HOME,(process.env.SHELL||'').endsWith('bash')?'.bash_profile':'.zprofile');
  const before=fs.existsSync(profile)?fs.readFileSync(profile,'utf8'):'';
  if(!before.includes('# Jcode Lite CLI')) journal.write(profile,before+'\n# Jcode Lite CLI\nexport PATH="$HOME/.local/bin:$PATH"\n');
  if(edition.startOnFresh || oldServers.length) await startServer(root,release.version,report);
  else report.push({step:'server',status:'READY: first launch performs provider login and starts the isolated server.'});
  if(edition.models.length) {
    const modelList=run(cli,['model','list','--json'],{env:{...process.env,JCODE_LITE_UPDATE_CHECK:'0'}});
    for(const model of edition.models) if(!modelList.includes(model)) throw Error(`Required configured model missing: ${model}`);
    report.push({step:'models',status:'PASS: local configured catalog only. Live entitlement not checked.'});
  } else report.push({step:'models',status:'USER CONFIGURED: no provider, model or credentials bundled.'});
  report.push({step:'update',status:'PASS',package:release.version});
} catch(error) {
  report.push({step:'update',status:'FAIL',message:error.message.split('\n')[0]});
  try {
    if(activated) {
      await stopServers(root,findServers(root).filter(s=>s.executable===path.join(target,'bin/jcode')),report);
      fs.renameSync(target,path.join(work,'failed-package'));
    }
    if(displaced && fs.existsSync(displaced) && !fs.existsSync(target)) fs.renameSync(displaced,target);
    journal.rollback();
    if(stopped && oldServers.length && state.current) await startServer(root,state.current,report);
    report.push({step:'rollback',status:'PASS: previous files restored; sessions were never removed.'});
  } catch(rollbackError) {report.push({step:'rollback',status:'FAIL',message:rollbackError.message.split('\n')[0]});}
  process.exitCode=1;
} finally {
  if(stage && fs.existsSync(stage)) fs.rmSync(stage,{recursive:true});
  atomic(path.join(work,'result.json'),JSON.stringify(report,null,2)+'\n');
  if(locked) fs.rmSync(lock,{recursive:true});
  for(const row of report) console.log(`[${row.step}] ${row.status}${row.version?' '+row.version:''}${row.message?' '+row.message:''}`);
  console.log(`Update report and private rollback journal: ${work}`);
  if(process.exitCode) console.log('Automatic bounded rollback was attempted. No unrestricted AI repair or extra macOS permission was silently authorized.');
}
