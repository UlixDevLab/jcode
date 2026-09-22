"""The builder and native build metadata must use Git's configured abbreviation."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class BuildRevisionTest(unittest.TestCase):
    def test_source_identity_matches_runtime_for_each_git_abbreviation(self):
        builders = [ROOT / 'build.sh']
        private_builder = ROOT.parent / 'jcode-lite/build.sh'
        if private_builder.exists():
            builders.append(private_builder)
        with tempfile.TemporaryDirectory(prefix='jcode-revision-') as directory:
            repo = Path(directory)
            def git(*args):
                return subprocess.check_output(['git', '-C', directory, *args], text=True).strip()
            git('init', '-q')
            git('config', 'user.name', 'Revision fixture')
            git('config', 'user.email', 'fixture@example.invalid')
            (repo / 'file').write_text('fixture\n')
            git('add', 'file')
            git('commit', '-qm', 'fixture')
            full = git('rev-parse', 'HEAD')
            for width in (7, 9, 12):
                git('config', 'core.abbrev', str(width))
                for builder in builders:
                    with self.subTest(builder=builder.parent.name, width=width):
                        assignment = next(line for line in builder.read_text().splitlines()
                                          if line.startswith('source_hash='))
                        result = subprocess.run(
                            ['bash', '-c', assignment + '\nprintf "%s" "$source_hash"'],
                            env={**os.environ, 'ROOT': directory},
                            text=True, capture_output=True, check=True)
                        self.assertEqual(result.stdout, full[:width])


if __name__ == '__main__':
    unittest.main()
