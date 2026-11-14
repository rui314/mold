#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Symbols of sections folded by ICF must remain in .symtab.
# https://github.com/rui314/mold/issues/1762

cat <<EOF | $CC -c -o $t/a.o -ffunction-sections -xc -
int foo() { return 42; }
int bar() { return 42; }
static int baz() { return 42; }
int main() { return baz(); }
EOF

$CC -B. -o $t/exe -Wl,-icf=all $t/a.o
nm $t/exe > $t/log
grep -q ' foo$' $t/log
grep -q ' bar$' $t/log
grep -q ' baz$' $t/log
grep -E ' (foo|bar|baz)$' $t/log | awk '{print $1}' | sort -u | wc -l | grep -qx 1
