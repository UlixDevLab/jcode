import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function verifyFiles(root) {
  for (const relative of fs.readFileSync(path.join(root, 'allowlist.txt'), 'utf8').split(/\r?\n/).filter(Boolean)) {
    if (relative.includes('\\') || path.posix.isAbsolute(relative) || /^[a-z]:/i.test(relative) || relative.split('/').some(p => p === '..' || p === '.' || !p)) throw new Error('Unsafe package path');
    if (!fs.statSync(path.join(root, relative)).isFile()) throw new Error(`Package file missing: ${relative}`);
  }
}

export function removeVersionTree(root, target) {
  root = path.resolve(root);
  target = path.resolve(target);
  if (path.dirname(target) !== root || !/^(?:\.staging-)?\d+\.\d+\.\d+(?:[.-][\w.-]+)?$/.test(path.basename(target))) throw new Error('Refusing non-version installation tree');
  if (fs.existsSync(target) && fs.lstatSync(target).isSymbolicLink()) throw new Error('Refusing linked installation tree');
  fs.rmSync(target, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [action, root, target] = process.argv.slice(2);
  if (action === 'verify' && root && !target) verifyFiles(root);
  else if (action === 'remove-version' && root && target) removeVersionTree(root, target);
  else throw new Error('Usage: install-files.mjs verify ROOT | remove-version INSTALL_ROOT VERSION_TREE');
}
