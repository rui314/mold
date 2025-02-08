#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fcommon -c -xc -o $t/a.o -
int foo;
EOF

cat <<EOF | $CC -fcommon -c -xc -o $t/b.o -
int foo;

int main() {
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o | not grep -Fq 'multiple common symbols'
$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-warn-common |& grep -Fq 'multiple common symbols'
