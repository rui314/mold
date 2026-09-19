#!/bin/bash
source "$(dirname "$0")"/common.inc

echo 'int main() {}' | $CC -c -xc - -o $t/a.o
$CC --ld-path=$mold $t/a.o -Wl,-fatal_warnings -o $t/exe

not $CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries,-fatal_warnings -o $t/exe 2> $t/log
grep -q 'duplicate libraries' $t/log

$CC --ld-path=$mold $t/a.o -lSystem \
  -Wl,-warn_duplicate_libraries,-fatal_warnings,-w -o $t/exe
