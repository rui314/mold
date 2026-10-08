#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--print-dependencies > $t/log
grep 'b\.o.*a\.o.*foo$' $t/log

# A blank line follows the comment header.
grep -A1 'compiler flag\.$' $t/log | tail -1 | grep -x ''
