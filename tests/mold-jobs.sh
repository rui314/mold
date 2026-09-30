#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

# An empty XDG_RUNTIME_DIR does not put the lock file in the current
# directory.
mold=$PWD/mold
mkdir -p $t/dir
rm -f $t/dir/mold-lock
(cd $t/dir; MOLD_JOBS=1 XDG_RUNTIME_DIR= $mold -r -o b.o ../a.o)
not test -e $t/dir/mold-lock
