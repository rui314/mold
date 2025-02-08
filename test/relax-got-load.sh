#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIC
#include <stdio.h>
extern char *msg;
void hello() { printf("%s\n", msg); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fPIC
char *msg = "Hello world";
void hello();
int main() { hello(); }
EOF

$CC -B. -o $t/exe1 $t/a.o $t/b.o
$QEMU $t/exe1 | grep 'Hello world'

$CC -B. -o $t/exe2 $t/a.o $t/b.o -Wl,--no-relax
$QEMU $t/exe2 | grep 'Hello world'

# On x86, GOTPCRELX is relaxed even with --no-relax
case $MACHINE in
aarch64 | riscv64 | s390x | loongarch64)
  $OBJDUMP -d $t/exe1 | grep -v exe1 > $t/log1
  $OBJDUMP -d $t/exe2 | grep -v exe2 > $t/log2
  not diff $t/log1 $t/log2 > /dev/null
  ;;
esac
