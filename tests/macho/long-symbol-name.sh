#!/bin/bash
source "$(dirname "$0")"/common.inc

# C++ template instantiations produce symbol names tens of thousands
# of bytes long; nothing in the link may truncate them. A 65536-byte
# name goes through the symbol table, the string table and the export
# trie whole.
name=$(head -c 65536 /dev/zero | tr '\0' 'x')
cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int $name(void) { return 42; }
int main() { printf("%d\n", $name()); }
EOF2
$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep -q '^42$'
nm $t/exe | awk '{print length($3)}' | grep -q '^65537$'
$CC --ld-path=$mold -dynamiclib -o $t/lib.dylib $t/a.o
dyld_info -exports $t/lib.dylib | awk '{print length($2)}' | grep -q '^65537$'
