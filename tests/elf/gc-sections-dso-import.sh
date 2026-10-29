#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A DSO symbol that only dead sections refer to is not imported.
cat <<EOF | $CC -fPIC -shared -o $t/a.so -xc -
#include <stdio.h>
void foo() { printf("foo\n"); }
void bar() {}
EOF

cat <<EOF | $CC -c -o $t/b.o -ffunction-sections -xc -
void foo();
void bar();
void dead() { bar(); }
int main() { foo(); }
EOF

$CC -B. -o $t/exe1 $t/b.o $t/a.so
readelf --dyn-syms $t/exe1 > $t/log1
grep -w foo $t/log1
grep -w bar $t/log1

$CC -B. -o $t/exe2 $t/b.o $t/a.so -Wl,--gc-sections -Wl,-rpath=$t
readelf --dyn-syms $t/exe2 > $t/log2
grep -w foo $t/log2
not grep -w bar $t/log2
$QEMU $t/exe2 | grep -w foo
