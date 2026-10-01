#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64's -sectorder <segment> <section> <path>, an order file for one
# section, ld-prime takes for an -order_file, whatever the section (an
# empty name too): its symbols lead their sections, wherever they are.
# Without its three arguments, or with an empty path, it is an error.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int d1 = 1, d2 = 2, d3 = 3;
void f1(void) { puts("f1"); }
void f2(void) { puts("f2"); }
void f3(void) { puts("f3"); }
int main(void) { f1(); f2(); f3(); return d1 + d2 + d3; }
EOF
printf '_f3\n_f2\n_d3\n' > $t/order

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-order_file,$t/order
mv $t/exe $t/exe1
for sect in __TEXT,__text __DATA,__data __FOO,__bar; do
  $CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectorder,$sect,$t/order
  cmp $t/exe $t/exe1
done
nm -n $t/exe | grep -A1 ' _f3$' | grep -q ' _f2$'
$mold -r -arch $ARCH -o $t/r.o $t/a.o -sectorder "" "" $t/order

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-sectorder,__TEXT,__text,$t/nonexistent 2> $t/log
grep -q "order file '$t/nonexistent' could not be opened" $t/log

not $mold -arch $ARCH -o $t/exe $t/a.o -sectorder __TEXT __text 2> $t/log
grep -q -- '-sectorder missing <segment> <section> <file-path>' $t/log
not $mold -arch $ARCH -o $t/exe $t/a.o -sectorder __TEXT __text '' 2> $t/log
grep -q -- '-sectorder missing <segment> <section> <file-path>' $t/log
