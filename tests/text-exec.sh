#!/bin/bash
source "$(dirname "$0")"/common.inc

# -text_exec gives code a segment of its own, __TEXT_EXEC (r-x): __text
# and the stubs move there, and __TEXT, left with the headers, strings
# and unwind info, becomes read-only. An arm64 kext implies it. (A macOS
# executable linked so doesn't launch; the kernel refuses it.)
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main(void) { puts("hi"); return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-text_exec
otool -l $t/exe | awk '$1 == "segname" { seg = $2 } $1 == "sectname" { sect = $2 }
  $1 == "initprot" { print seg, $2 } $1 == "size" && sect { print sect, seg; sect = "" }' > $t/segs
grep -q '^__TEXT 0x00000001$' $t/segs
grep -q '^__TEXT_EXEC 0x00000005$' $t/segs
grep -q '^__text __TEXT_EXEC$' $t/segs
grep -q '^__stubs __TEXT_EXEC$' $t/segs
grep -q '^__cstring __TEXT$' $t/segs
