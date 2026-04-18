#!/bin/bash
. $(dirname $0)/common.inc

on_qemu && skip
[ $MACHINE = riscv64 -o $MACHINE = riscv32 -o $MACHINE = sparc64 ] && skip
test_cflags -gdwarf-5 -g || skip

# Generate many translation units that all reference common types and
# names so the per-name CU index list contains entries from multiple
# threads. This stresses the parallel writer in --gdb-index that fills
# in (type, cu) tuples for each name, which must produce a deterministic
# output regardless of thread scheduling.
objs=
for i in $(seq 1 24); do
  cat <<EOF > $t/u$i.c
#include <stdio.h>
struct s$i { int x; };
int fn$i(struct s$i *p) { return printf("%d\n", p->x); }
EOF
  $CC -c -o $t/u$i.o $t/u$i.c -fPIC -g -ggnu-pubnames
  objs="$objs $t/u$i.o"
done

$CC -B. -shared -o $t/out1.so -Wl,--gdb-index $objs
$CC -B. -shared -o $t/out2.so -Wl,--gdb-index $objs
$CC -B. -shared -o $t/out3.so -Wl,--gdb-index $objs
$CC -B. -shared -o $t/out4.so -Wl,--gdb-index $objs
$CC -B. -shared -o $t/out5.so -Wl,--gdb-index $objs

cmp $t/out1.so $t/out2.so
cmp $t/out1.so $t/out3.so
cmp $t/out1.so $t/out4.so
cmp $t/out1.so $t/out5.so
