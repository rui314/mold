#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image orders a segment's sections by their kind, as mold's
# sort_output_sections does: code first, then data, then zero fill,
# each kind in the order its sections' first members come in the
# inputs, the linker's own sections after the inputs' - in any segment.
order() {
  otool -l $t/$1 | awk '$1 == "sectname" { s = $2; next }
    $1 == "segname" && s != "" { printf "%s,%s ", $2, s; s = "" }'
}

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__zz
.quad 1
.section __DATA,__yy
.quad 1
.section __TEXT,__zcode,regular,pure_instructions
_zc: ret
.zerofill __DATA,__zbss,_zb,8,3
.data
.quad 2
.section __DATA,__mycode,regular,pure_instructions
_dc: ret
.section __FOO,__x
.quad 1
.section __FOO,__b,regular,pure_instructions
_fb: ret
.text
.globl _main
_main: ret
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
order exe > $t/order
grep -q '__TEXT,__text __TEXT,__zcode __TEXT,__zz ' $t/order
grep -q '__DATA,__mycode __DATA,__yy __DATA,__data __DATA,__zbss ' $t/order
grep -q '__FOO,__b __FOO,__x ' $t/order

# The thread-local template is one block dyld copies for each thread:
# its initial values come last among the file-backed sections and its
# zero fill first among the zero-fill ones, with nothing in between.
cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
__thread int tv = 1;
__thread int tz;
int bss[4];
int data = 5;
int main() { printf("%d %d %d %d\n", tv, tz, bss[0], data); }
EOF
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA,__late
.quad 1
.zerofill __DATA,__lbss,_lb,8,3
EOF
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/c.o
$t/exe2 | grep '^1 0 0 5$'
order exe2 | grep -o '__DATA,[a-z_]*' > $t/order2
tr '\n' ' ' < $t/order2 | grep -q '__DATA,__late .*__DATA,__thread_data __DATA,__thread_bss '
[ "$(sed -n '/__thread_bss/,$p' $t/order2 | sort | tr '\n' ' ')" = \
  '__DATA,__common __DATA,__lbss __DATA,__thread_bss ' ]

# Read-only data goes to __DATA_CONST, before __DATA, unless
# -no_data_const keeps it in __DATA.
cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>
const char *const names[] = {"a", "b"};
int counter = 1;
int main() { printf("%s %d\n", names[1], counter); }
EOF
$CC --ld-path=$mold -o $t/exe3 $t/d.o
$t/exe3 | grep '^b 1$'
order exe3 > $t/order3
grep -q '__DATA_CONST,__const .*__DATA,__data ' $t/order3
$CC --ld-path=$mold -o $t/exe4 $t/d.o -Wl,-no_data_const
$t/exe4 | grep '^b 1$'
order exe4 > $t/order4
not grep -q __DATA_CONST $t/order4
grep -q '__DATA,__const' $t/order4

# Renamed segments keep the kinds' order within them.
$CC --ld-path=$mold -o $t/exe5 $t/a.o \
  -Wl,-rename_segment,__DATA,__XD -Wl,-rename_segment,__FOO,__XF
order exe5 > $t/order5
grep -q '__XD,__mycode __XD,__yy __XD,__data __XD,__zbss ' $t/order5
grep -q '__XF,__b __XF,__x ' $t/order5
