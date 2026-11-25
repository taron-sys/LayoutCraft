#!/usr/bin/env python3
"""Deterministic .zip / .tar.gz of a directory, the same on every platform (docs/36-packaging.md).

    archive.py zip|targz <out> <dir> --epoch <unix time>

The entries are <dir's name>/..., sorted by name, with the given modification time, owner root and no extra attributes.
File modes are kept (the executable bit of a binary survives); every directory is 0755. Standard library only.
"""
import argparse
import gzip
import os
import stat
import sys
import tarfile
import time
import zipfile


def entries(root):
    root = os.path.normpath(root)
    parent = os.path.dirname(root)
    found = [(os.path.basename(root), root, True)]
    for base, dirs, files in os.walk(root):
        for name in dirs + files:
            path = os.path.join(base, name)
            found.append((os.path.relpath(path, parent).replace(os.sep, "/"), path, os.path.isdir(path)))
    return sorted(found, key=lambda e: e[0])


def make_zip(out, root, epoch):
    stamp = time.gmtime(max(epoch, 315532800))[:6]  # a zip cannot store dates before 1980
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for rel, path, is_dir in entries(root):
            if is_dir:
                continue
            info = zipfile.ZipInfo(rel, stamp)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = (stat.S_IMODE(os.stat(path).st_mode) | stat.S_IFREG) << 16
            with open(path, "rb") as f:
                z.writestr(info, f.read())


def make_targz(out, root, epoch):
    with open(out, "wb") as raw, gzip.GzipFile("", "wb", 9, raw, mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode="w", format=tarfile.PAX_FORMAT) as tar:
            for rel, path, is_dir in entries(root):
                info = tar.gettarinfo(path, arcname=rel)
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.mtime = epoch
                info.mode = 0o755 if is_dir else stat.S_IMODE(info.mode)
                info.pax_headers = {}
                if is_dir:
                    tar.addfile(info)
                else:
                    with open(path, "rb") as f:
                        tar.addfile(info, f)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=["zip", "targz"])
    parser.add_argument("out")
    parser.add_argument("directory")
    parser.add_argument("--epoch", type=int, required=True)
    args = parser.parse_args()
    if not os.path.isdir(args.directory):
        sys.exit(f"archive.py: {args.directory} is not a directory")
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    (make_zip if args.kind == "zip" else make_targz)(args.out, args.directory, args.epoch)


if __name__ == "__main__":
    main()
