#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void unused_member() {}
EOF

rm -f $t/lib.a
ar rcs $t/lib.a $t/a.o $t/b.o

cat <<EOF | $CC -o $t/main.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a -Wl,-t > $t/log
grep -q '/main.o$' $t/log
grep -q 'lib.a(a.o)$' $t/log
grep -q 'libSystem' $t/log
# The unused member took no part in the link.
! grep -q 'b.o' $t/log || false
