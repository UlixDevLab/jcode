import fs from 'node:fs';
import path from 'node:path';
import {atomic,privateDir,sha,edition} from './runtime.mjs';

export class Journal {
  constructor(dir) {this.dir=privateDir(dir);this.entries=[];}
  write(file,data,mode=0o600) {
    if(fs.existsSync(file) && fs.lstatSync(file).isSymbolicLink()) throw Error(`Refusing to replace symlink: ${file}`);
    const entry={file,exists:fs.existsSync(file),backup:path.join(this.dir,String(this.entries.length))};
    if(entry.exists) {entry.mode=fs.statSync(file).mode&0o777;fs.copyFileSync(file,entry.backup);fs.chmodSync(entry.backup,0o600);}
    this.entries.push(entry);
    atomic(path.join(this.dir,'journal.json'),JSON.stringify(this.entries,null,2));
    atomic(file,data,mode);
  }
  rollback() {
    for(const item of [...this.entries].reverse()) {
      if(item.exists) atomic(item.file,fs.readFileSync(item.backup),item.mode);
      else if(fs.existsSync(item.file)) fs.unlinkSync(item.file);
    }
  }
}
export function aliasLine(line,root,home) {
  if(!/jcode/i.test(line)) return {line,status:null};
  const match=line.match(/^(\s*alias\s+)([A-Za-z_][A-Za-z0-9_-]*)=(['"])(.*?)\3\s*$/);
  if(!match) return {line,status:'SKIP: complex alias/function or unrelated line'};
  let value=match[4];
  if((value.startsWith('"')&&value.endsWith('"'))||(value.startsWith("'")&&value.endsWith("'"))) value=value.slice(1,-1);
  value=value.replace(/^\$HOME(?=\/)/,home).replace(/^~(?=\/)/,home);
  const relative=path.relative(root,value);
  if(!/^\d+\.\d+\.\d+[-A-Za-z0-9.]*\/(?:bin\/jcode|jcode-lite|jcode-free)$/.test(relative)) return {line,status:'SKIP: not a direct alias to this Lite installation'};
  return {line:`${match[1]}${match[2]}='"$HOME/.local/bin/${edition.cli}"'`,status:`UPDATED: ${match[2]}`};
}
export function migrateAliases(root,journal,report) {
  for(const name of ['.zshrc','.zprofile','.bashrc','.bash_profile','.profile']) {
    const file=path.join(process.env.HOME,name);
    if(!fs.existsSync(file)) continue;
    if(fs.lstatSync(file).isSymbolicLink()) {report.push({step:'aliases',file:name,status:'SKIP: symlink'});continue;}
    const before=fs.readFileSync(file,'utf8');
    const after=before.split('\n').map(line=>{
      const result=aliasLine(line,root,process.env.HOME);
      if(result.status) report.push({step:'aliases',file:name,status:result.status});
      return result.line;
    }).join('\n');
    if(after!==before) journal.write(file,after,fs.statSync(file).mode&0o777);
  }
  report.push({step:'current-shell',status:'NOTICE: open a new terminal or reload its profile and run hash -r. A child process cannot modify parent-shell aliases.'});
}
export function managedFile(source,dest,prior,journal,report) {
  if(fs.existsSync(dest)) {
    if(fs.lstatSync(dest).isSymbolicLink()) throw Error(`Managed destination is a symlink: ${dest}`);
    if(sha(source)===sha(dest)) return;
    if(!prior || !fs.existsSync(prior) || sha(prior)!==sha(dest)) {
      report.push({step:'rules',file:path.basename(dest),status:'PRESERVED: local edit conflicts with new preset. Compare package preset with your file.'});return;
    }
  }
  journal.write(dest,fs.readFileSync(source),fs.statSync(source).mode&0o777);
}
export function migrateHome(root,app,prior,journal,report) {
  const home=privateDir(path.join(root,'home'));
  edition.migrateConfig(home,app,journal);
  for(const file of ['swarm-prompt.md']) managedFile(path.join(app,'preset',file),path.join(home,file),prior&&path.join(prior,'preset',file),journal,report);
  for(const prefix of ['skills','roles','knowledge-os/templates']) {
    const base=path.join(app,'preset',prefix);
    for(const relative of fs.readdirSync(base,{recursive:true}).filter(p=>fs.statSync(path.join(base,p)).isFile())) {
      const destination=prefix!=='knowledge-os/templates'?path.join(home,prefix,relative):path.join(home,'templates/knowledge-os',relative);
      managedFile(path.join(base,relative),destination,prior&&path.join(prior,'preset',prefix,relative),journal,report);
    }
  }
  const overlay=path.join(home,'prompt-overlay.md');
  if(fs.existsSync(overlay)) report.push({step:'rules',file:'prompt-overlay.md',status:'PRESERVED: user/legacy overlay retained. Project overrides may alter cognitive routing.'});
}
