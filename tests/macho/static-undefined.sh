#!/bin/bash
source "$(dirname "$0")"/common.inc

# -undefined dynamic_lookup, suppress and -U leave a symbol for dyld
# to look up at run time. A -static image has no dyld, so ld-prime lets
# none stay undefined, whatever those options say, weak references
# included.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl __start
.p2align 2
__start:
  ret
.data
.p2align 3
.quad _missing
.weak_reference _weak
.quad _weak
EOF

not $mold -arch $ARCH -static -e __start $t/a.o -o $t/exe1 2> $t/log1
grep -q _missing $t/log1
grep -q _weak $t/log1

not $mold -arch $ARCH -static -e __start -undefined dynamic_lookup $t/a.o -o $t/exe2 2> $t/log2
grep -q _missing $t/log2

not $mold -arch $ARCH -static -e __start -U _missing -U _weak $t/a.o -o $t/exe3 2> $t/log3
grep -q _missing $t/log3
grep -q _weak $t/log3

not $mold -arch $ARCH -static -e __start -flat_namespace -undefined suppress $t/a.o \
  -o $t/exe4 2> $t/log4
grep -q _missing $t/log4

not $mold -arch $ARCH -static -e __start -undefined dynamic_lookup -fixup_chains $t/a.o \
  -o $t/exe5 2> $t/log5
grep -q _missing $t/log5
