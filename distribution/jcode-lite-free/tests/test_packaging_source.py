import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('packaging_source', Path(__file__).resolve().parents[1] / 'support/verify-packaging-source.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PackagingSourceTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='jcode-packaging-source-')
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.git('init', '-q')
        self.git('config', 'core.abbrev', '7')
        self.git('config', 'user.name', 'Packaging fixture')
        self.git('config', 'user.email', 'fixture@example.invalid')
        self.commit('src/main.rs', 'original runtime')
        self.base = self.git('rev-parse', 'HEAD')

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.repo), *args], text=True).strip()

    def commit(self, name, content):
        file = self.repo / name
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(content)
        self.git('add', name)
        self.git('commit', '-qm', 'fixture')

    def test_clean_packaging_only_delta_retains_binary_revision(self):
        self.commit('distribution/jcode-lite-free/windows/install.ps1', 'fixed installer')
        self.commit('README.md', 'beginner guide')
        self.assertEqual(module.validate(self.repo, self.base), self.base[:7])

    def test_runtime_and_build_changes_require_a_new_binary(self):
        for name in ['src/main.rs', 'Cargo.lock', 'Cargo.toml', 'crates/runtime/src/lib.rs', 'build.rs', '.cargo/config.toml']:
            with self.subTest(name=name):
                before = self.git('rev-parse', 'HEAD')
                self.commit(name, 'runtime change')
                with self.assertRaisesRegex(ValueError, 'runtime/build inputs changed'):
                    module.validate(self.repo, before)

    def test_dirty_or_unrelated_source_rejected(self):
        (self.repo / 'untracked').write_text('not committed')
        with self.assertRaisesRegex(ValueError, 'clean packaging checkout'):
            module.validate(self.repo, self.base)
        self.git('add', 'untracked')
        with self.assertRaisesRegex(ValueError, 'clean packaging checkout'):
            module.validate(self.repo, self.base)
        self.git('commit', '-qm', 'fixture')
        later = self.git('rev-parse', 'HEAD')
        self.git('checkout', '--detach', self.base)
        with self.assertRaisesRegex(ValueError, 'ancestor'):
            module.validate(self.repo, later)


if __name__ == '__main__':
    unittest.main()
