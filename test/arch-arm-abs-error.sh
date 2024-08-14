#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xassembler - -mthumb
.globl foo
foo = 3;
EOF

cat <<EOF | $CC -fno-PIC -c -o $t/b.o -xc - -mthumb 2> /dev/null || skip
#include <stdio.h>
extern char foo;
int main() { printf("foo=%p\n", &foo); }
EOF

$CC -o $t/exe -pie $t/a.o $t/b.o >& /dev/null && skip

! $CC -B. -o $t/exe -pie $t/a.o $t/b.o >& $t/log
grep -q 'recompile with -fPIC' $t/log
