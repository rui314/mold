#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

not $CC -B. -o $t/exe1 $t/a.o $t/a.o $t/b.o >& $t/log1
grep -q 'duplicate symbol.*: foo$' $t/log1

not $CC -B. -o $t/exe2 $t/a.o $t/a.o $t/b.o -Wl,-gc-sections >& $t/log2
grep -q 'duplicate symbol.*: foo$' $t/log2
