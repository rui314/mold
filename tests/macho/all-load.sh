#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int three() { return 3; }
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
int five() { return 5; }
EOF2

cat <<EOF2 | $CC -o $t/c.o -c -xc -
int main() { return 0; }
EOF2

rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/a.o $t/b.o

$CC --ld-path=$mold -o $t/exe $t/c.o $t/libfoo.a -Wl,-all_load
nm $t/exe > $t/syms
grep -q _three $t/syms
grep -q _five $t/syms

$CC --ld-path=$mold -o $t/exe2 $t/c.o -Wl,-force_load,$t/libfoo.a
nm $t/exe2 > $t/syms2
grep -q _three $t/syms2
grep -q _five $t/syms2

# -all_load exempts clang's runtime archive (libclang_rt.*.a, which the
# compiler driver adds to every link): a member of it is loaded only
# when referenced, as with ld64.
mkdir -p $t/rt
cp $t/libfoo.a $t/rt/libclang_rt.osx.a
$CC --ld-path=$mold -o $t/exe3 $t/c.o $t/rt/libclang_rt.osx.a -Wl,-all_load
nm $t/exe3 > $t/syms3
not grep -q _three $t/syms3
not grep -q _five $t/syms3
