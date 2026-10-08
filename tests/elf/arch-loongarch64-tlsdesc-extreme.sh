#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -O2 -mcmodel=extreme $tlsdesc_opt -c -o $t/a.o -xc - || skip
_Thread_local int foo = 3;
static _Thread_local int bar = 5;
int get_foo() { return foo; }
int get_bar() { return bar; }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>
int get_foo();
int get_bar();
int main() { printf("%d %d\n", get_foo(), get_bar()); }
EOF

# A static executable can't use TLSDESC, so the sequences are relaxed to LE.
if test_cflags -static; then
  $CC -B. -static -o $t/exe1 $t/a.o $t/b.o
  $QEMU $t/exe1 | grep -x '3 5'
fi

# Otherwise, the extreme code model's TLSDESC sequences are not relaxed.
$CC -B. -o $t/exe2 $t/a.o $t/b.o
readelf -rW $t/exe2 | grep -F R_LARCH_TLS_DESC64

$CC -B. -shared -o $t/c.so $t/a.o
$CC -B. -o $t/exe3 $t/b.o $t/c.so -Wl,-rpath=$t
$OBJDUMP -d $t/c.so | grep -A8 '<get_foo>:' > $t/log
grep -F lu52i.d $t/log
not grep -F pcaddi $t/log

if supports_tlsdesc; then
  $QEMU $t/exe2 | grep -x '3 5'
  $QEMU $t/exe3 | grep -x '3 5'
fi
