#!/bin/sh
set -e

PREFIX=${PREFIX:-/usr/local}
LIBDIR=${LIBDIR:-$PREFIX/lib}
LIBEXECDIR=${LIBEXECDIR:-$PREFIX/libexec}
MANDIR=${MANDIR:-$PREFIX/share/man}
DOCDIR=${DOCDIR:-$PREFIX/share/doc/mold}

srcdir=$(CDPATH= cd "$(dirname "$0")" && pwd)
artifact_dir="${CARGO_TARGET_DIR:-$srcdir/target}/release"

if [ ! -x "$artifact_dir/mold" ] ||
  [ ! -f "$artifact_dir/mold-wrapper.so" ]; then
  echo "install-mold.sh: release artifacts are missing" >&2
  echo "Run 'cargo build --release' first." >&2
  exit 1
fi

case $LIBEXECDIR in
  "$PREFIX"/*) ;;
  *)
    echo "install-mold.sh: LIBEXECDIR must be a directory under $PREFIX" >&2
    exit 1
    ;;
esac

bindir="$DESTDIR$PREFIX/bin"
libdir="$DESTDIR$LIBDIR/mold"
libexecdir="$DESTDIR$LIBEXECDIR/mold"
mandir="$DESTDIR$MANDIR/man1"
docdir="$DESTDIR$DOCDIR"

install -d "$bindir" "$libdir" "$libexecdir" "$mandir" "$docdir"
install -m 755 "$artifact_dir/mold" "$bindir"
install -m 755 "$artifact_dir/mold-wrapper.so" "$libdir"
install -m 644 "$srcdir/docs/mold.1" "$mandir"
install -m 644 "$srcdir/LICENSE" "$docdir"

ln -sf mold "$bindir/ld.mold"
ln -sf mold.1 "$mandir/ld.mold.1"

# The ld symlink for GCC's -B option points to the executable with a relative
# path, so that the installed tree can be moved as a whole. Each directory
# below $PREFIX becomes a "..".
up=$(echo "${LIBEXECDIR#"$PREFIX"/}/mold" | sed 's,[^/][^/]*,..,g')
ln -sf "$up/bin/mold" "$libexecdir/ld"
