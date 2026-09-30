#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image orders a segment's sections by fixed ranks for the ones
# ld-prime knows and by input order for the rest. In __TEXT, __text
# leads and the other code sections follow it in input order, ahead of
# the stubs and every data section; in __DATA the Objective-C sections
# lead and __data takes its input place among the unknown sections.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __TEXT,__zz
.quad 1
.section __DATA,__yy
.quad 1
.section __TEXT,__zcode,regular,pure_instructions
_zc: ret
.data
.quad 2
.section __DATA,__objc_data
.quad 3
.text
.globl _main
_main: ret
.subsections_via_symbols
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
otool -l $t/exe | awk '$1 == "sectname" { s = $2; next }
  $1 == "segname" && s != "" { printf "%s,%s ", $2, s; s = "" }' > $t/order
grep -q '__TEXT,__text __TEXT,__zcode __TEXT,__zz ' $t/order
grep -q '__DATA,__objc_data __DATA,__yy __DATA,__data ' $t/order
