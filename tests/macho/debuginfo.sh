#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 > $t/main.c
#include <stdio.h>
int compute(int x) { return x * 7; }
int main() {
  printf("%d\n", compute(6));
}
EOF2

$CC -g -c $t/main.c -o $t/main.o
$CC --ld-path=$mold -g -o $t/exe $t/main.o
$t/exe | grep '^42$'

# The output should have OSO stabs pointing at the object file
nm -pa $t/exe > $t/stabs
grep -q 'OSO.*main.o' $t/stabs
grep -q 'FUN _compute' $t/stabs

# lldb should be able to set a source-level breakpoint and hit it
lldb -b -o 'b compute' -o run -o 'p x' $t/exe > $t/lldb.log 2>&1 || true
grep -q 'stop reason = breakpoint' $t/lldb.log
grep -q '(int) 6' $t/lldb.log
