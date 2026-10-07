#!/bin/bash
source "$(dirname "$0")"/common.inc

# crt1.o's tables for dyld and the C runtime, __DATA,__dyld (before
# macOS 10.6) and __DATA,__program_vars, stay in __DATA under their own
# names, which old dyld and the C runtime find them by, whatever the
# target.
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
$RUN $t/exe | grep '^5$'
otool -l $t/exe | awk '/sectname/ { s = $2 } /segname/ { if (s && $2 == "__DATA") print s; s = "" }' \
  | sort > $t/sects
cat > $t/expected <<EOF
__aaa
__data
__dyld
__la_symbol_ptr
__program_vars
EOF
diff $t/expected $t/sects
otool -s __DATA __program_vars $t/exe | grep -E '00000002 00000000|02 00 00 00 00 00 00 00'
otool -s __DATA __dyld $t/exe | grep -E '00000003 00000000|03 00 00 00 00 00 00 00'
