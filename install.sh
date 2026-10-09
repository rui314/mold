#!/bin/sh
# Installs the release build as $PREFIX/bin/mold and creates the
# ld64.mold symlink next to it, the name to give clang's --ld-path or
# swiftc's -use-ld. Mirrors mold's install-mold.sh, where the
# symlink is ld.mold.
set -e

PREFIX=${PREFIX:-/usr/local}

srcdir=$(CDPATH= cd "$(dirname "$0")" && pwd)
artifact_dir="${CARGO_TARGET_DIR:-$srcdir/target}/release"

if [ ! -x "$artifact_dir/mold" ]; then
  echo "install.sh: release artifacts are missing" >&2
  echo "Run 'cargo build --release' first." >&2
  exit 1
fi

bindir="$DESTDIR$PREFIX/bin"
docdir="$DESTDIR$PREFIX/share/doc/mold-macho"

install -d "$bindir" "$docdir"
install -m 755 "$artifact_dir/mold" "$bindir"
install -m 644 "$srcdir/LICENSE" "$docdir"

ln -sf mold "$bindir/ld64.mold"
