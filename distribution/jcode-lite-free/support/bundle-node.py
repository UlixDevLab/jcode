#!/usr/bin/env python3
"""Bundle the existing Lite-pinned Node runtime. Build host only, no recipient Python."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--archive', type=Path)
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text())
    version = manifest['version']
    if manifest['platform'] != 'darwin-arm64' or not re.fullmatch(r'\d+\.\d+\.\d+', version):
        parser.error('Only the pinned darwin-arm64 Node runtime is supported')
    destination = args.stage / 'runtime/node'
    if destination.exists():
        parser.error('Node staging destination must not already exist')
    args.stage.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.node-bundle-', dir=args.stage) as temp:
        temp = Path(temp)
        archive = args.archive
        if archive is None:
            url = manifest['archive']['url']
            expected_url = f'https://nodejs.org/dist/v{version}/node-v{version}-darwin-arm64.tar.gz'
            if url != expected_url:
                parser.error('Node download URL differs from the pinned official release')
            archive = temp / 'node.tar.gz'
            subprocess.run(['curl', '--fail', '--location', '--proto', '=https',
                            '--proto-redir', '=https', '--tlsv1.2', '--connect-timeout', '10',
                            '--max-time', '180', '--output', str(archive), url], check=True)
        with archive.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        if digest != manifest['archive']['sha256']:
            parser.error('Node runtime checksum mismatch')
        with tarfile.open(archive, 'r:gz') as bundle:
            bundle.extractall(temp, filter='data')
        source = temp / f'node-v{version}-darwin-arm64'
        for required in ['bin/node', 'lib/node_modules/npm/bin/npm-cli.js', 'LICENSE']:
            if not (source / required).is_file():
                parser.error(f'Node archive is missing {required}')
        # Materialize npm's relative symlinks, as the package allowlist contains files.
        shutil.copytree(source, destination, symlinks=False)
        npm = destination / 'bin/npm'
        npm.write_text('#!/bin/sh\nset -eu\nBIN_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\n'
                       'exec "$BIN_DIR/node" "$BIN_DIR/../lib/node_modules/npm/bin/npm-cli.js" "$@"\n')
        npm.chmod(0o755)
        (destination / 'bin/node').chmod(0o755)
        shutil.copy2(args.manifest, args.stage / 'node-runtime.json')
    print(f'Bundled verified Node {version} for darwin-arm64')


if __name__ == '__main__':
    main()
