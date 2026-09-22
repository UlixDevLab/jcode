import importlib.util
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('create_zip', Path(__file__).resolve().parents[1] / 'support/create-zip.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PortableZipTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='jcode-zip-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.stage = self.root / 'stage'
        self.stage.mkdir()
        (self.stage / 'payload').write_bytes(b'payload')
        self.listing = self.root / 'files'
        self.listing.write_text('payload\n')
        self.output = self.root / 'package.zip'

    def test_exact_contents_and_exclusive_creation(self):
        module.create(self.stage, self.output, self.listing)
        before = self.output.read_bytes()
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(archive.namelist(), ['payload'])
            self.assertEqual(archive.read('payload'), b'payload')
        with self.assertRaises(FileExistsError):
            module.create(self.stage, self.output, self.listing)
        self.assertEqual(before, self.output.read_bytes())

    def test_unsafe_missing_and_duplicate_entries_fail_before_output(self):
        for listing in ('../outside\n', '/absolute\n', 'a\\b\n', 'missing\n', 'payload\npayload\n'):
            with self.subTest(listing=listing):
                self.listing.write_text(listing)
                with self.assertRaises(ValueError):
                    module.create(self.stage, self.output, self.listing)
                self.assertFalse(self.output.exists())

    def test_checksum_reporter_needs_only_python(self):
        builder = Path(__file__).resolve().parents[1] / 'build.sh'
        source = builder.read_text()
        reporter = source.split("<<'CHECKSUM'\n", 1)[1].split('\nCHECKSUM', 1)[0]
        payload = self.root / 'archive with spaces.zip'
        payload.write_bytes(b'abc')
        result = subprocess.run([sys.executable, '-', str(payload)], input=reporter,
                                text=True, capture_output=True, check=True)
        self.assertEqual(result.stdout,
                         'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'
                         + '  ' + str(payload) + '\n')

    def test_symlinks_rejected(self):
        (self.stage / 'linked').symlink_to(self.stage / 'payload')
        self.listing.write_text('linked\n')
        with self.assertRaises(ValueError):
            module.create(self.stage, self.output, self.listing)
        self.assertFalse(self.output.exists())


if __name__ == '__main__':
    unittest.main()
