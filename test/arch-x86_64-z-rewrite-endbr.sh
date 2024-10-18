#!/bin/bash
. $(dirname $0)/common.inc

test_cflags -fcf-protection || skip

cat <<EOF | $CC -o $t/a.o -c -xc - -fno-function-sections -O -fcf-protection
int foo() { return 3; }
int bar() { return foo(); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -ffunction-sections -O -fcf-protection
int main() {}
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o
$OBJDUMP -dr $t/exe1 > $t/log1

grep -A1 '<foo>:' $t/log1 | grep -q endbr64
grep -A1 '<bar>:' $t/log1 | grep -q endbr64
grep -A1 '<main>:' $t/log1 | grep -q endbr64

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,-z,rewrite-endbr
$OBJDUMP -dr $t/exe2 > $t/log2

grep -A1 '<foo>:' $t/log2 | grep -q nop
grep -A1 '<bar>:' $t/log2 | grep -q nop
grep -A1 '<main>:' $t/log2 | grep -q endbr64
