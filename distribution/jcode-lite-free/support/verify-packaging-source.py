#!/usr/bin/env python3
"""Allow explicit binary reuse only across a clean, packaging-only Git delta."""
from pathlib import Path
import subprocess
import sys

PREFIXES = ('distribution/jcode-lite-free/', 'distribution/jcode-lite/common/preset/',
            'docs/', '.github/')
FILES = {'README.md', '.gitattributes'}


def validate(repo, revision):
    def git(*args):
        return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()
    commit = git('rev-parse', '--verify', revision + '^{commit}')
    if git('status', '--porcelain', '--untracked-files=all'):
        raise ValueError('Binary reuse requires a clean packaging checkout')
    if subprocess.run(['git', '-C', str(repo), 'merge-base', '--is-ancestor', commit, 'HEAD'],
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
        raise ValueError('Binary source must be an ancestor of the packaging source')
    changed = git('diff', '--name-only', '--no-renames', commit, 'HEAD').splitlines()
    forbidden = [name for name in changed if name not in FILES and not name.startswith(PREFIXES)]
    if forbidden:
        raise ValueError('Binary reuse rejected: runtime/build inputs changed: ' + ', '.join(forbidden))
    return git('rev-parse', '--short', commit)


if __name__ == '__main__':
    try:
        if len(sys.argv) != 3:
            raise ValueError('Usage: verify-packaging-source.py REPO BINARY_SOURCE_COMMIT')
        print(validate(Path(sys.argv[1]), sys.argv[2]))
    except (ValueError, subprocess.CalledProcessError) as error:
        raise SystemExit(str(error))
