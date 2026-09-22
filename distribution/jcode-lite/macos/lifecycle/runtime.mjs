import fs from 'node:fs';
import edition from './edition.mjs';
export {edition};
import path from 'node:path';
import crypto from 'node:crypto';
import {execFileSync} from 'node:child_process';

export const versionPattern = /^\d+\.\d+\.\d+(?:-[A-Za-z0-9.]+)?$/;
export const sha = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
export function privateDir(dir) {
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, {recursive:true, mode:0o700});
  const s = fs.lstatSync(dir);
  if (!s.isDirectory() || s.isSymbolicLink() || s.uid !== process.getuid()) throw Error(`Unsafe directory: ${dir}`);
  fs.chmodSync(dir, 0o700);
  return fs.realpathSync(dir);
}
export function installation() {
  return path.join(process.env.HOME, 'Library/Application Support/LeGrin', edition.rootName);
}
export function runtime(root) {
  root = privateDir(root);
  privateDir(path.join(root, 'runtime'));
  const id = crypto.createHash('sha256').update(root).digest('hex').slice(0,16);
  const sockets = privateDir(`/tmp/jcl-${process.getuid()}-${id}`);
  return {root, runtime:path.join(root,'runtime'), socket:path.join(sockets,'jcode.sock')};
}
export function readState(root) {
  if (!fs.existsSync(path.join(root,'state.json'))) return {current:'', previous:''};
  const state = JSON.parse(fs.readFileSync(path.join(root,'state.json')));
  for (const key of ['current','previous']) if (state[key] && !versionPattern.test(state[key])) throw Error('Invalid Lite state version');
  return state;
}
export function atomic(file, data, mode=0o600) {
  privateDir(path.dirname(file));
  if (fs.existsSync(file) && fs.lstatSync(file).isSymbolicLink()) throw Error(`Refusing symlink: ${file}`);
  const temp = `${file}.new-${process.pid}`;
  fs.writeFileSync(temp,data,{mode,flag:'wx'});
  fs.renameSync(temp,file);
}
export function run(exe,args,options={}) {
  return execFileSync(exe,args,{encoding:'utf8',timeout:30000,maxBuffer:4*1024*1024,stdio:['ignore','pipe','pipe'],...options});
}
export function packageFiles(source) {
  const names=fs.readFileSync(path.join(source,'allowlist.txt'),'utf8').trim().split('\n');
  if (new Set(names).size!==names.length) throw Error('Duplicate package entries');
  for(const name of names) {
    if(!name || path.isAbsolute(name) || name.split('/').some(p=>p==='..'||p==='.') || name.includes('\\')) throw Error('Unsafe package entry');
    const file=path.join(source,name), stat=fs.lstatSync(file);
    if(!stat.isFile() || stat.isSymbolicLink() || !fs.realpathSync(file).startsWith(fs.realpathSync(source)+path.sep)) throw Error('Unsafe package file');
  }
  return names;
}
export function copyPackage(source,dest) {
  privateDir(dest);
  for(const name of packageFiles(source)) {
    const from=path.join(source,name), to=path.join(dest,name);
    fs.mkdirSync(path.dirname(to),{recursive:true,mode:0o700});
    fs.copyFileSync(from,to,fs.constants.COPYFILE_EXCL);
    fs.chmodSync(to,fs.statSync(from).mode & 0o777);
  }
}
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  console.log(runtime(process.argv[2]).socket);
}
