#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = sh4 ] && skip

seq 1    1000  | sed 's/.*/void func&() {}/' > $t/a0.c
seq 1001 2000  | sed 's/.*/void func&() {}/' > $t/a1.c
seq 2001 3000  | sed 's/.*/void func&() {}/' > $t/a2.c
seq 3001 4000  | sed 's/.*/void func&() {}/' > $t/a3.c
seq 4001 5000  | sed 's/.*/void func&() {}/' > $t/a4.c
seq 5001 6000  | sed 's/.*/void func&() {}/' > $t/a5.c
seq 6001 7000  | sed 's/.*/void func&() {}/' > $t/a6.c
seq 7001 8000  | sed 's/.*/void func&() {}/' > $t/a7.c
seq 8001 9000  | sed 's/.*/void func&() {}/' > $t/a8.c
seq 9001 10000 | sed 's/.*/void func&() {}/' > $t/a9.c

$CC -c -fPIC -o $t/a0.o $t/a0.c
$CC -c -fPIC -o $t/a1.o $t/a1.c
$CC -c -fPIC -o $t/a2.o $t/a2.c
$CC -c -fPIC -o $t/a3.o $t/a3.c
$CC -c -fPIC -o $t/a4.o $t/a4.c
$CC -c -fPIC -o $t/a5.o $t/a5.c
$CC -c -fPIC -o $t/a6.o $t/a6.c
$CC -c -fPIC -o $t/a7.o $t/a7.c
$CC -c -fPIC -o $t/a8.o $t/a8.c
$CC -c -fPIC -o $t/a9.o $t/a9.c

$CC -B. -o $t/b.so -shared $t/a0.o $t/a1.o $t/a2.o $t/a3.o $t/a4.o $t/a5.o $t/a6.o $t/a7.o $t/a8.o $t/a9.o

seq 1 10000 | sed 's/.*/void func&();/' > $t/c.c
echo 'int main() {' >> $t/c.c
seq 1 10000 | sed 's/.*/func&();/' >> $t/c.c
echo '}' >> $t/c.c

$CC -c -o $t/d.o $t/c.c
$CC -B. -o $t/exe $t/d.o $t/b.so
$QEMU $t/exe
