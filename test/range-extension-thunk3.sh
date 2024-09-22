#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = sh4 ] && skip

seq 1 10000 | sed 's/.*/void func&() {}/' > $t/a.c
$CC -B. -o $t/b.so -shared $t/a.c

seq 1 10000 | sed 's/.*/void func&();/' > $t/c.c
echo 'int main() {' >> $t/c.c
seq 1 10000 | sed 's/.*/func&();/' >> $t/c.c
echo '}' >> $t/c.c

$CC -c -o $t/d.o $t/c.c
$CC -B. -o $t/exe $t/d.o $t/b.so
$QEMU $t/exe
