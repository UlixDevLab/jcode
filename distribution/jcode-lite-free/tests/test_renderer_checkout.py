"""Approved renderer bytes must survive native Windows Git checkout unchanged."""
import hashlib
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
DIRECTORY = 'distribution/jcode-lite/common/preset/knowledge-os/renderer/'
APPROVED = {
    'export-lite.mjs': '101ef35e1230ce2ef839bcd50429f4e3d78832878d13710dd3054969f6866748',
    'renderer-template.html': 'c8c0e6de1e412082f3ec5404d310e2823005206694845e1e5350566dae8976fe',
}


class RendererCheckoutTest(unittest.TestCase):
    def test_repository_assets_match_approved_bytes(self):
        for name, expected in APPROVED.items():
            with self.subTest(name=name):
                self.assertEqual(hashlib.sha256((ROOT / DIRECTORY / name).read_bytes()).hexdigest(), expected)

    def test_real_git_autocrlf_checkout_preserves_approved_bytes(self):
        with tempfile.TemporaryDirectory(prefix='jcode-renderer-checkout-') as directory:
            prefix = Path(directory).as_posix() + '/'
            subprocess.run(['git', '-C', str(ROOT), '-c', 'core.autocrlf=true',
                            'checkout-index', '--prefix=' + prefix,
                            *(DIRECTORY + name for name in APPROVED)], check=True)
            for name, expected in APPROVED.items():
                with self.subTest(name=name):
                    data = (Path(directory) / DIRECTORY / name).read_bytes()
                    self.assertEqual(hashlib.sha256(data).hexdigest(), expected)


if __name__ == '__main__':
    unittest.main()
