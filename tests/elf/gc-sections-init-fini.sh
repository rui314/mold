#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = riscv64 -o $MACHINE = riscv32 ] && skip
[[ $MACHINE = loongarch* ]] && skip

# musl libc does not support init/fini on ARM
# https://github.com/rui314/mold/issues/951
[[ $MACHINE = arm* || $MACHINE = aarch64 ]] && is_musl && skip

# The --init and --fini functions are not exported, so only DT_INIT and
# DT_FINI keep them alive under --gc-sections.
cat <<EOF | $CC -c -fPIC -ffunction-sections -o $t/a.o -xc -
#include <stdio.h>

__attribute__((visibility("hidden"))) void init() {
  printf("init\n");
}

__attribute__((visibility("hidden"))) void fini() {
  printf("fini\n");
}

void keep() {}
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
void keep();

int main() {
  keep();
}
EOF

$CC -B. -o $t/c.so -shared $t/a.o -Wl,-init,init,-fini,fini,--gc-sections
$CC -B. -o $t/exe $t/b.o $t/c.so
$QEMU $t/exe > $t/log

grep init $t/log
grep fini $t/log
