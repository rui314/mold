#!/usr/bin/env bash
. $(dirname $0)/common.inc

lto_library=$(dirname "$(xcrun -f clang)")/../lib/libLTO.dylib

cat <<EOF | $CC -flto -c -xc - -o $t/a.o
int twice(int);
__attribute__((visibility("hidden"))) int add1(int x) { return x + 1; }
int add1_twice(int x) { return twice(add1(x)); }
EOF
echo 'int twice(int x) { return x * 2; }' | $CC -flto -c -xc - -o $t/b.o
echo 'int twice(int x) { return x * 2; }' | $CC -c -xc - -o $t/bn.o
cat <<EOF | $CC -c -xc - -o $t/main.o
#include <stdio.h>
int add1_twice(int);
int main() { printf("%d\n", add1_twice(20)); }
EOF

# A -r link of bitcode alone merges the modules into one bitcode file
# (with Darwin's wrapper header), which the final link then optimizes
# as a whole. Every symbol survives, unless an export list says not.
$mold -r -arch $ARCH -lto_library $lto_library -o $t/r1.o $t/a.o $t/b.o
test "$(xxd -l 4 -p $t/r1.o)" = dec0170b
nm $t/r1.o > $t/r1.nm
grep -q ' T _add1_twice$' $t/r1.nm
grep -q ' T _twice$' $t/r1.nm
grep -q ' T _add1$' $t/r1.nm
$CC --ld-path=$mold -flto -o $t/exe1 $t/main.o $t/r1.o
$RUN $t/exe1 | grep -q '^42$'

$mold -r -arch $ARCH -lto_library $lto_library -o $t/r2.o $t/a.o $t/b.o \
  -exported_symbol _add1_twice
nm $t/r2.o > $t/r2.nm
grep -q ' T _add1_twice$' $t/r2.nm
not grep -q ' T _twice$' $t/r2.nm

# With a Mach-O object among the inputs, LTO compiles the bitcode, and
# the object it makes joins the -r output like any other. A private
# extern turns local there, as -r makes of any.
$mold -r -arch $ARCH -lto_library $lto_library -o $t/r3.o $t/a.o $t/bn.o
test "$(xxd -l 4 -p $t/r3.o)" = cffaedfe
nm -m $t/r3.o > $t/r3.nm
grep -q '(__TEXT,__text) external _add1_twice$' $t/r3.nm
grep -q '(__TEXT,__text) external _twice$' $t/r3.nm
grep -q 'non-external (was a private external) _add1$' $t/r3.nm
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/r3.o
$RUN $t/exe3 | grep -q '^42$'
