#!/bin/bash
source "$(dirname "$0")"/common.inc

# -remove_swift_reflection_metadata_sections drops Swift's field
# descriptors, associated type records and the names they give
# (__swift5_fieldmd, __swift5_assocty, __swift5_reflstr, in any
# segment), but not the type references, from a final image and a -r
# output alike.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.section __TEXT,__swift5_reflstr,regular,no_dead_strip
_rs1: .asciz "field"
.section __TEXT,__swift5_fieldmd,regular,no_dead_strip
.p2align 2
_fm1: .long 0
.section __TEXT,__swift5_assocty,regular,no_dead_strip
.p2align 2
_at1: .long 0
.section __TEXT,__swift5_typeref,regular,no_dead_strip
_tr1: .asciz "Si"
.section __DATA,__swift5_reflstr
_rs2: .asciz "x"
.subsections_via_symbols
EOF

sects() { otool -l $1 | awk '$1 == "sectname" { printf "%s ", $2 }'; }

$CC --ld-path=$mold -o $t/exe $t/a.o
sects $t/exe | grep -q __swift5_fieldmd
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-remove_swift_reflection_metadata_sections
[ "$(sects $t/exe | grep -o '__swift5_[a-z]*' | tr '\n' ' ')" = '__swift5_typeref ' ]
$mold -r -arch $ARCH -o $t/r.o $t/a.o -remove_swift_reflection_metadata_sections
[ "$(sects $t/r.o)" = '__text __swift5_typeref ' ]

# What still refers to them fails the link: ld-prime reports the first
# reference by address, as it writes it.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
.section __TEXT,__const
.p2align 2
_desc: .long 0
  .long _fm1 - _desc
_desc2: .long _fm1 - _desc2
.section __TEXT,__swift5_fieldmd,regular,no_dead_strip
.p2align 2
_fm1: .long 0
.subsections_via_symbols
EOF
not $CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-remove_swift_reflection_metadata_sections 2> $t/log
grep -q "fixup error (kind=diff32) at '_desc'+0x4 from b.o, target '_fm1' does not have address" $t/log
[ "$(grep -c 'fixup error' $t/log)" = 1 ]
