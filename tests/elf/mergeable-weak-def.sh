#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A weak definition in a mergeable section must not override a strong
# definition in another file. We once attached a section piece to such a
# symbol even if it resolved to the other file's definition.

cat <<EOF | $CC -c -o $t/a.o -xc -
char msg[4] = "AAA";
EOF

cat <<EOF | $CC -c -o $t/b.o -xassembler -
.section .rodata.str1.1, "aMS", %progbits, 1
.weak msg
msg:
  .string "BBB"
EOF

cat <<EOF | $CC -c -o $t/c.o -xc -
#include <stdio.h>
extern char msg[];
int main() { printf("%s\n", msg); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o $t/c.o
$QEMU $t/exe | grep '^AAA$'
