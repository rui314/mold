#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A section group whose flag word is 0 is not a COMDAT group. Its members
# are never deduplicated, but they are kept or discarded as a unit, so -r
# output must keep the group. annobin emits such groups.
cat <<'EOF' | $CC -c -o $t/a.o -xc -
__asm__(".pushsection .data.foo,\"awG\",%progbits,grp\n"
        ".globl foo\n"
        ".balign 4\n"
        "foo: .long 42\n"
        ".popsection\n"
        ".pushsection .data.bar,\"awG\",%progbits,grp\n"
        ".globl bar\n"
        ".balign 8\n"
        "bar: .dc.a foo\n"
        ".popsection\n");
EOF

cat <<'EOF' | $CC -c -o $t/b.o -xc -
#include <stdio.h>
extern int *bar;
int main() { printf("%d\n", *bar); }
EOF

readelf --section-groups $t/a.o | grep '^group section .* \[grp\]'

./mold -r -o $t/c.o $t/a.o
readelf --section-groups $t/c.o > $t/log
grep '^group section .* \[grp\] contains 3 sections' $t/log

# Each SHF_GROUP section must be a member of a group.
readelf -SW $t/c.o | grep -E ' [A-Za-z]*G[A-Za-z]* +[0-9]+ +[0-9]+ +[0-9]+$' |
  sed -E 's/^ *\[ *([0-9]+)\].*/\1/' > $t/flagged
[ -s $t/flagged ]
while read i; do grep -E "^ +\[ +$i\] " $t/log; done < $t/flagged

$CC -B. -o $t/exe $t/b.o $t/c.o
$QEMU $t/exe | grep '^42$'
