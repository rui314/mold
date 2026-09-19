#!/bin/bash
source "$(dirname "$0")"/common.inc

# A __eh_frame whose CFI length runs past the section is an error
# ("malformed __eh_frame section: CFI length too long" in ld64), not a
# crash.
cat <<EOF2 | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__eh_frame
.byte 1
.text
.globl _main
.p2align 2
_main: ret
EOF2
not $mold -r -arch $ARCH -o $t/r.o $t/a.o 2> $t/err
grep -q 'malformed __eh_frame' $t/err

cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__eh_frame
.long 0x1000
.long 0
.text
.globl _main
.p2align 2
_main: ret
EOF2
not $mold -r -arch $ARCH -o $t/r2.o $t/b.o 2> $t/err2
grep -q 'malformed __eh_frame' $t/err2
