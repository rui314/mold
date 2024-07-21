#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -B. -shared -o $t/libfoo.so -xc -
void foo() {}
EOF

cat <<EOF | $CC -B. -shared -o $t/libbar.so -xc -
void foo();
void bar() { foo(); }
EOF

cat <<EOF | $CC -c -o $t/a.o -c -xc -
int bar();
int main() { bar(); }
EOF

$CC -B. -o $t/exe1 $t/a.o -Wl,--no-allow-shlib-undefined -L$t -lfoo -lbar

! $CC -B. -o $t/exe2 $t/a.o -Wl,--no-allow-shlib-undefined -L$t -lbar >& $t/log || false
grep -Fq 'undefined symbol: foo' $t/log
