#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime applies -rename_section and -rename_segment to the sections
# it synthesizes as to the input ones: the stubs, the GOT (by its
# __DATA_CONST name), __init_offsets and -sectcreate's - but not
# __unwind_info, which stays in __TEXT. -rename_segment __TEXT takes
# __text along but leaves a dynamic image's mach header in __TEXT.
# (ld-prime also leaves an empty __TEXT,__text there.)
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
const char *const msg = "hello";
__attribute__((constructor)) static void init(void) {}
int main() { puts(msg); }
EOF

# Code without unwind info, which may leave __TEXT.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _main
_main:
  ret
.data
.quad 1
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { print $2 "," s; s = "" }'
}

echo blob > $t/blob
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-rename_section,__TEXT,__stubs,__TEXT_EXEC,__stubs \
  -Wl,-rename_section,__DATA_CONST,__got,__GOT,__got \
  -Wl,-rename_section,__TEXT,__unwind_info,__FOO,__unwind_info \
  -Wl,-sectcreate,__SC,__sc,$t/blob -Wl,-rename_segment,__SC,__SC2
sects $t/exe > $t/sects
grep -qx '__TEXT_EXEC,__stubs' $t/sects
grep -qx '__GOT,__got' $t/sects
grep -qx '__TEXT,__unwind_info' $t/sects
grep -qx '__SC2,__sc' $t/sects
not grep -q '__TEXT,__stubs\|__FOO' $t/sects
# (__TEXT_EXEC is executable, as in ld64.)
$RUN $t/exe | grep -q hello

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-rename_section,__TEXT,__init_offsets,__INIT,__io
grep -qx '__INIT,__io' <(sects $t/exe2)

$CC --ld-path=$mold -o $t/exe3 $t/b.o -Wl,-rename_section,__TEXT,__text,__TEXT_EXEC,__text
grep -qx '__TEXT_EXEC,__text' <(sects $t/exe3)
not grep -q '__TEXT,' <(sects $t/exe3)

$CC --ld-path=$mold -o $t/exe4 $t/b.o -Wl,-rename_segment,__TEXT,__TTT
grep -qx '__TTT,__text' <(sects $t/exe4)
otool -l $t/exe4 | grep -q 'segname __TEXT$'
