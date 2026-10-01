#!/bin/bash
source "$(dirname "$0")"/common.inc

# crt1.o's tables for dyld and the C runtime, __DATA,__dyld (before
# macOS 10.6) and __DATA,__program_vars, lead __DATA in input order,
# ahead of the lazy pointers and of the sections before them in input
# order, whatever the target.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__aaa
.quad 1
.section __DATA,__program_vars
.quad 2
.section __DATA,__dyld
.quad 3
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int x = 5;
int main() { printf("%d\n", x); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -mmacosx-version-min=11.0
otool -l $t/exe | awk '/sectname/ { s = $2 } /segname/ { if (s && $2 == "__DATA") print s; s = "" }' \
  > $t/sects
cat > $t/expected <<EOF
__program_vars
__dyld
__la_symbol_ptr
__aaa
__data
EOF
diff $t/expected $t/sects
