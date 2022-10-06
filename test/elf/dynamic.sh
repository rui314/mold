#!/bin/bash
. $(dirname $0)/common.inc

echo '.globl main; main:' | $CC -o $t/a.o -c -x assembler -

$CC -B. -o $t/exe $t/a.o

readelf --dynamic $t/exe > $t/log
grep -Eq 'Shared library:.*\blibc\b' $t/log

readelf -W --dyn-syms --use-dynamic $t/exe > $t/log2
grep -Eq 'FUNC\s+GLOBAL\s+DEFAULT\s+UND\s+__libc_start_main' $t/log2

cat <<EOF | $CC -c -fPIC -o $t/b.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
}
EOF
