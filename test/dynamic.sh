#!/bin/bash
. $(dirname $0)/common.inc

echo '.globl main; main:' | $CC -o $t/a.o -c -x assembler -

$CC -B. -o $t/exe $t/a.o

readelf --dynamic $t/exe | grep -Eq 'Shared library:.*\blibc\b'

readelf -W --dyn-syms --use-dynamic $t/exe | \
  grep -Eq 'FUNC\s+GLOBAL\s+DEFAULT.*UND\s+__libc_start'
