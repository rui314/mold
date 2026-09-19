#!/bin/sh
set -e

PREFIX=${PREFIX:-/usr/local}

srcdir=$(CDPATH= cd "$(dirname "$0")" && pwd)
artifact_dir="${CARGO_TARGET_DIR:-$srcdir/target}/release"

if [ ! -x "$artifact_dir/mold" ] ||
  [ ! -f "$artifact_dir/mold-wrapper.so" ]; then
  echo "install-mold.sh: release artifacts are missing" >&2
  echo "Run 'cargo build --release' first." >&2
  exit 1
fi

bindir="$DESTDIR$PREFIX/bin"
libdir="$DESTDIR$PREFIX/lib/mold"
libexecdir="$DESTDIR$PREFIX/libexec/mold"
mandir="$DESTDIR$PREFIX/share/man/man1"
docdir="$DESTDIR$PREFIX/share/doc/mold"

install -d "$bindir" "$libdir" "$libexecdir" "$mandir" "$docdir"
install -m 755 "$artifact_dir/mold" "$bindir"
install -m 755 "$artifact_dir/mold-wrapper.so" "$libdir"
install -m 644 "$srcdir/docs/mold.1" "$mandir"
install -m 644 "$srcdir/LICENSE" "$docdir"

ln -sf mold "$bindir/ld.mold"
ln -sf mold "$bindir/ld64.mold"
ln -sf ../../bin/mold "$libexecdir/ld"
ln -sf mold.1 "$mandir/ld.mold.1"
