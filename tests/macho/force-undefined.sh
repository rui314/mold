#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int keepme() { return 1; }
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
int main() { return 0; }
EOF2

rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/a.o

# Without -u the member is not needed; with it, it must be linked.
$CC --ld-path=$mold -o $t/exe $t/b.o $t/libfoo.a
nm $t/exe > $t/syms
not grep -q _keepme $t/syms

$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/libfoo.a -Wl,-u,_keepme
nm $t/exe2 > $t/syms2
grep -q _keepme $t/syms2
