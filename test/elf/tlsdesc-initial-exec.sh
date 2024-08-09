#!/bin/bash
. $(dirname $0)/common.inc

supports_tlsdesc || skip

cat <<EOF | $GCC -fPIC -fPIC -c -o $t/a.o -xc - $tlsdesc_opt
_Thread_local int foo = 5;
EOF

$CC -B. -shared -o $t/b.so $t/a.o

cat <<EOF | $GCC -fPIC -fPIC -c -o $t/c.o -xc - $tlsdesc_opt
extern _Thread_local int foo;
int get_foo1() { return foo; }
EOF

cat <<EOF | $GCC -fPIC -fPIE -c -o $t/d.o -xc - $tlsdesc_opt
#include <stdio.h>

extern _Thread_local int foo;
int get_foo1();
int get_foo2() { return foo; }

int main() {
  printf("%d %d %d\n", foo, get_foo1(), get_foo2());
}
EOF

$CC -B. -o $t/exe1 $t/c.o $t/d.o $t/b.so
$QEMU $t/exe1 | grep -q '^5 5 5$'

readelf -Wr $t/exe1 > $t/log1
! grep -Eq 'TLS.?DESC|unrecognized:' $t/log1 || false

$CC -B. -o $t/exe1 $t/c.o $t/d.o $t/b.so -Wl,--no-relax
$QEMU $t/exe1 | grep -q '^5 5 5$'

readelf -Wr $t/exe1 > $t/log2
grep -Eq 'TLS.?DESC|unrecognized:' $t/log2
