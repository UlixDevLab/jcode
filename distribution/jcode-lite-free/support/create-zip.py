#!/usr/bin/env python3
"""Create a package using Python when the native host has no Unix zip command."""
from pathlib import Path, PurePosixPath
import sys
import zipfile


def create(stage, output, listing):
    stage = Path(stage).resolve(strict=True)
    names = Path(listing).read_text().splitlines()
    seen = set()
    for name in names:
        relative = PurePosixPath(name)
        if not name or relative.is_absolute() or '..' in relative.parts or '\\' in name or name in seen:
            raise ValueError('Unsafe or duplicate package entry')
        source = stage / name
        if source.is_symlink() or not source.is_file() or not source.resolve().is_relative_to(stage):
            raise ValueError('Package entry must be a regular staged file')
        seen.add(name)
    # Exclusive creation preserves immutable existing artifacts.
    with zipfile.ZipFile(output, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as archive:
        for name in names:
            archive.write(stage / name, name)


if __name__ == '__main__':
    create(*sys.argv[1:])
