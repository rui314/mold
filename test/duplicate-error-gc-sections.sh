#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

not $CC -B. -o $t/exe1 $t/a.o $t/a.o $t/b.o |&
  grep -q 'duplicate symbol.*: foo$'

not $CC -B. -o $t/exe2 $t/a.o $t/a.o $t/b.o -Wl,-gc-sections |&
  grep -q 'duplicate symbol.*: foo$'
