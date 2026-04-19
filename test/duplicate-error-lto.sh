#!/bin/bash
. $(dirname $0)/common.inc

nm mold | grep '__tsan_init' && skip
test_cflags -flto || skip

cat <<EOF | $CC -o $t/a.o -c -xc - -flto
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -flto
int main() {}
EOF

not $CC -B. -o $t/exe1 $t/a.o $t/a.o $t/b.o -flto |&
  grep 'duplicate symbol.*: foo$'
