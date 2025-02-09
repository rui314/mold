#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CXX -o $t/a.o -c -xc++ - -fno-PIC
#include <stdio.h>
extern char _etext[];
int main() {
  printf("%p\n", _etext);
}
EOF

$CXX -B. -o $t/exe $t/a.o -no-pie
readelf --unwind $t/exe | grep "$($QEMU $t/exe) .*cantunwind"
