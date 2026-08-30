#!/usr/bin/env bash
. $(dirname $0)/common.inc

# We support ELFv2 only for little-endian PPC64. A big-endian ELFv2 file
# must be rejected instead of silently linked as ELFv1.
# https://github.com/rui314/mold/issues/1498

[ $MACHINE = ppc64 ] || skip

cat <<EOF | $CC -mabi=elfv2 -c -o $t/a.o -xc - 2> /dev/null || skip
int main() {}
EOF

readelf -h $t/a.o | grep -q abiv2 || skip

! ./mold -m elf64ppc -o $t/exe $t/a.o 2> $t/log
grep 'unknown machine type' $t/log
