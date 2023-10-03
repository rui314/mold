#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

rm -f $t/b.a
ar rcs $t/b.a $t/a.o

cat <<EOF | $CC -o $t/c.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC -o $t/exe $t/c.o $t/b.a $t/b.a

! $CC -o $t/exe $t/c.o -Wl,--push-state,--whole-archive \
  $t/b.a $t/b.a -Wl,--pop-state 2> $t/log || false

grep -q 'multiple definition of .*foo' $t/log
