"""Build-time bundling contract and dependency-free macOS recipient bootstrap."""
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

import test_install_flow

ROOT = Path(__file__).resolve().parents[1]
HELPER = ROOT / 'support/bundle-node.py'


class BundleNodeTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='free-node-bundle-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.archive = self.root / 'node.tar.gz'
        with tarfile.open(self.archive, 'w:gz') as tar:
            for name, content in {
                'bin/node': b'node-fixture',
                'lib/node_modules/npm/bin/npm-cli.js': b'npm-fixture',
                'LICENSE': b'Node license fixture',
            }.items():
                entry = tarfile.TarInfo(f'node-v22.17.0-darwin-arm64/{name}')
                entry.size, entry.mode = len(content), 0o755
                tar.addfile(entry, io.BytesIO(content))
            entry = tarfile.TarInfo('node-v22.17.0-darwin-arm64/bin/npm')
            entry.type = tarfile.SYMTYPE
            entry.linkname = '../lib/node_modules/npm/bin/npm-cli.js'
            tar.addfile(entry)
        self.manifest = self.root / 'node-runtime.json'
        self.manifest.write_text(json.dumps({
            'version': '22.17.0', 'platform': 'darwin-arm64',
            'archive': {'url': 'https://nodejs.org/unused',
                        'sha256': hashlib.sha256(self.archive.read_bytes()).hexdigest()},
        }))
        self.stage = self.root / 'stage'
        self.stage.mkdir()

    def run_bundle(self):
        return subprocess.run([sys.executable, str(HELPER), '--manifest', str(self.manifest),
                               '--archive', str(self.archive), '--stage', str(self.stage)],
                              text=True, capture_output=True)

    def test_materializes_verified_runtime_and_portable_npm_entrypoint(self):
        result = self.run_bundle()
        self.assertEqual(result.returncode, 0, result.stderr)
        runtime = self.stage / 'runtime/node'
        self.assertEqual((runtime / 'bin/node').read_bytes(), b'node-fixture')
        self.assertFalse((runtime / 'bin/npm').is_symlink())
        self.assertIn('../lib/node_modules/npm/bin/npm-cli.js', (runtime / 'bin/npm').read_text())
        self.assertEqual((runtime / 'LICENSE').read_bytes(), b'Node license fixture')
        self.assertEqual((self.stage / 'node-runtime.json').read_bytes(), self.manifest.read_bytes())

    def test_checksum_mismatch_never_installs_runtime(self):
        self.archive.write_bytes(b'corrupt download')
        result = self.run_bundle()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('checksum', result.stderr.lower())
        self.assertFalse((self.stage / 'runtime/node').exists())


@unittest.skipUnless(sys.platform == 'darwin', 'actual macOS installer script')
class BundledRecipientTest(unittest.TestCase):
    def test_install_and_reinstall_without_global_node_or_python(self):
        fixture = test_install_flow.MacInstallFlowTest()
        fixture.setUp()
        self.addCleanup(fixture.tearDown)
        package = fixture.package
        runtime = package / 'runtime/node/bin'
        runtime.mkdir(parents=True)
        host_node = shutil.which('node')
        self.assertIsNotNone(host_node)
        # Synthetic package runtime delegates to the verified test-host Node.
        # It proves bootstrap/PATH selection, not release Node provenance.
        (runtime / 'node').write_text(f'#!/bin/sh\nexec "{host_node}" "$@"\n')
        (runtime / 'node').chmod(0o755)
        (package / 'preset/mcp/node_modules').mkdir(exist_ok=True)
        (package / 'preset/mcp/node_modules/fixture-marker').write_text('no network needed')
        files = sorted(str(p.relative_to(package)) for p in package.rglob('*') if p.is_file())
        (package / 'allowlist.txt').write_text('\n'.join(files) + '\n')
        blockers = fixture.base / 'blocked-dependencies'
        blockers.mkdir()
        for command in ['node', 'python3', 'npm']:
            executable = blockers / command
            executable.write_text('#!/bin/sh\necho "unexpected host dependency" >&2\nexit 97\n')
            executable.chmod(0o755)
        env = {**fixture.env, 'PATH': f'{blockers}:/usr/bin:/bin',
               'JCODE_LITE_FREE_INSTALL_NO_LAUNCH': '1'}
        for _ in range(2):
            result = subprocess.run([str(package / 'install.command')], env=env,
                                    text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        cli = fixture.home / '.local/bin/jcodef'
        result = subprocess.run([str(cli), 'version'], env=env, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('FAKE_JCODE --no-update version', result.stdout)
        self.assertFalse((fixture.home / '.jcode').exists())
