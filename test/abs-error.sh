#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = aarch64 ] && skip
[ $MACHINE = ppc64 ] && skip
[ $MACHINE = ppc64le ] && skip
[ $MACHINE = s390x ] && skip
[[ $MACHINE = loongarch* ]] && skip

cat <<EOF | $CC -fPIC -c -o $t/a.o -xassembler -
.globl foo
foo = 3;
EOF

cat <<EOF | $CC -fno-PIC -c -o $t/b.o -xc -
#include <stdio.h>
extern char foo;
int main() { printf("foo=%p\n", &foo); }
EOF

not $CC -B. -o $t/exe -pie $t/a.o $t/b.o -Wl,-z,text >& $t/log
grep -q 'recompile with -fPIC' $t/log
