#!/usr/bin/env python3
"""Native Windows CI follow-through: add SHA-pinned official Node to a Free artifact."""
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import sys
import tempfile
import urllib.request
import zipfile

URL = 'https://nodejs.org/dist/v22.17.0/node-v22.17.0-win-x64.zip'
EXPECTED = '721ab118a3aac8584348b132767eadf51379e0616f0db802cc1e66d7f0d98f85'

def main():
    if sys.platform != 'win32':
        raise SystemExit('Windows artifacts must be assembled on native Windows')
    archive = Path(sys.argv[1]).resolve()
    with urllib.request.urlopen(URL, timeout=120) as response:
        raw = response.read()
    if hashlib.sha256(raw).hexdigest() != EXPECTED:
        raise SystemExit('Official Node checksum mismatch')
    with zipfile.ZipFile(archive) as source:
        files = {item.filename: (source.read(item), item.external_attr)
                 for item in source.infolist() if not item.is_dir()}
    with zipfile.ZipFile(io.BytesIO(raw)) as node:
        prefix = 'node-v22.17.0-win-x64/'
        for item in node.infolist():
            if item.is_dir():
                continue
            if not item.filename.startswith(prefix):
                raise SystemExit('Unexpected Node archive root')
            relative = item.filename[len(prefix):]
            if '..' in PurePosixPath(relative).parts or relative.startswith('/') or '\\' in relative:
                raise SystemExit('Unsafe Node archive member')
            files['runtime/node/' + relative] = (node.read(item), 0o100644 << 16)
    files['node-runtime.json'] = ((json.dumps({'version':'22.17.0','platform':'win-x64',
        'archive':{'url':URL,'sha256':EXPECTED}}, indent=2)+'\n').encode(), 0o100644 << 16)
    files['allowlist.txt'] = (('\n'.join(sorted(files))+'\n').encode(), 0o100644 << 16)
    handle, temporary = tempfile.mkstemp(prefix='free-node-', suffix='.zip', dir=archive.parent)
    try:
        with os.fdopen(handle,'wb') as stream:
            with zipfile.ZipFile(stream,'w',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as output:
                for name,(content,mode) in sorted(files.items()):
                    item=zipfile.ZipInfo(name)
                    item.external_attr=mode
                    item.compress_type=zipfile.ZIP_DEFLATED
                    output.writestr(item,content)
        os.replace(temporary,archive)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    print(json.dumps({'archive':archive.name,'sha256':hashlib.file_digest(archive.open('rb'),'sha256').hexdigest(),
                      'bundled_node':'22.17.0','files':len(files)}))

if __name__ == '__main__':
    main()
