#!/usr/bin/env bash
. $(dirname $0)/common.inc

# -fPIC code loads the symbols' addresses from the GOT, and those GOT
# entries need base relocations in a PIE even though the symbols are
# absolute in .symtab.
cat <<EOF | $CC -o $t/a.o -c -xc -fPIC -
#include <stdio.h>
#include <string.h>

extern char __ehdr_start[];
extern char __executable_start[];

int main() {
  printf("%d %d\n", !memcmp(__ehdr_start, "\177ELF", 4),
         !memcmp(__executable_start, "\177ELF", 4));
}
EOF

$CC -B. -pie -o $t/exe1 $t/a.o
$QEMU $t/exe1 | grep '^1 1$'

$CC -B. -pie -o $t/exe2 $t/a.o -Wl,-no-relax
$QEMU $t/exe2 | grep '^1 1$'
