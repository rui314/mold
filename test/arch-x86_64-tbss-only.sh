#!/bin/bash
. $(dirname $0)/common.inc

# Test if grep supports backreferences
echo abab | grep -Eq '(ab)\1' || skip

cat <<EOF | $CC -o $t/a.o -c -xc -
__thread char foo;

__attribute__((section(".data.rel.ro.bar"), aligned(16*1024)))
char bar;

int main() {}
EOF

$CC -B. -o $t/exe $t/a.o
$QEMU $t/exe

readelf -W --segments $t/exe | grep -Eq 'TLS +0x000([^ ][^ ][^ ]) 0x[^ ]+\1 '
