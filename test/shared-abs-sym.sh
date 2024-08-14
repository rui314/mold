#!/bin/bash
. $(dirname $0)/common.inc

# LD_PRELOAD may not work in some environments, so try to test it
# with the default linker first
cat <<EOF | $CC -fPIC -shared -o $t/a.so -xc -
int foo() { return 3; }
EOF

cat <<EOF | $CC -fPIC -shared -o $t/b.so -xc -
int foo() { return 5; }
EOF

cat <<EOF | $CC -fPIC -c -o $t/c.o -xc -
#include <stdio.h>
int foo();
int main() { printf("foo=%d\n", foo()); }
EOF

$CC -o $t/exe1 -pie $t/c.o $t/a.so
$QEMU $t/exe1 | grep -q 'foo=3'
LD_PRELOAD=$t/b.so $QEMU $t/exe1 | grep -q 'foo=5' || skip

# If LD_PRELOAD works, continue
cat <<EOF | $CC -B. -fPIC -shared -o $t/d.so -xassembler -
.globl foo
foo = 3;
EOF

cat <<EOF | $CC -B. -fPIC -shared -o $t/e.so -xassembler -
.globl foo
foo = 5;
EOF

cat <<EOF | $CC -fPIC -c -o $t/f.o -xc -
#include <stdio.h>
extern char foo;
int main() { printf("foo=%p\n", &foo); }
EOF

# This test fails with older glibc
$CC -B. -o $t/exe2 -pie $t/f.o $t/d.so 2> /dev/null || skip
$QEMU $t/exe2 | grep -q 'foo=0x3' || skip
LD_PRELOAD=$t/e.so $QEMU $t/exe2 | grep -q 'foo=0x5'

$CC -B. -o $t/exe3 -pie $t/f.o $t/d.so
$QEMU $t/exe3 | grep -q 'foo=0x3'
LD_PRELOAD=$t/e.so $QEMU $t/exe3 | grep -q 'foo=0x5'

$CC -B. -o $t/exe4 -no-pie $t/f.o $t/d.so
$QEMU $t/exe4 | grep -q 'foo=0x3'
LD_PRELOAD=$t/e.so $QEMU $t/exe4 | grep -q 'foo=0x5'
