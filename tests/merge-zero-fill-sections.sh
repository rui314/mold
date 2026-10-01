#!/bin/bash
source "$(dirname "$0")"/common.inc

# -merge_zero_fill_sections merges each segment's zero-fill sections
# into one named __zerofill, in a final image and a -r output alike: the
# objects' in input order, each object's in section order, and in a
# final image the common symbols after them. The merge comes before
# -rename_section, which so renames __zerofill but no longer __bss.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.zerofill __DATA,__zfb,_zb1,128,4
.zerofill __DATA,__bss,_bss1,32,3
.zerofill __DATA,__zfa,_za1,64,3
.comm _com1,40,3
.zerofill __FOO,__bss,_foo1,16,2
.data
.globl _d
_d: .quad 1
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.zerofill __DATA,__bss,_bss2,24,5
.comm _com2,8,3
.text
.globl _main
.p2align 2
_main:
  ret
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s { print $2 "," s; s = "" }' |
    tr '\n' ' '
}
syms() {
  nm -n $1 | awk '$3 !~ /^(__mh_execute_header|_main|_d|ltmp[0-9]*)$/ { printf "%s ", $3 }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-merge_zero_fill_sections
[ "$(sects $t/exe)" = '__TEXT,__text __DATA,__data __DATA,__zerofill __FOO,__zerofill ' ]
[ "$(syms $t/exe)" = '_zb1 _bss1 _za1 _bss2 _com1 _com2 _foo1 ' ]
otool -l $t/exe | grep -A10 'sectname __zerofill' | grep -q 'align 2^5'
otool -l $t/exe | grep -A10 'sectname __zerofill' | grep -q 'flags 0x00000001'

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -Wl,-merge_zero_fill_sections \
  -Wl,-rename_section,__DATA,__bss,__DATA,__mybss -Wl,-rename_section,__DATA,__zerofill,__DATA,__zz
[ "$(sects $t/exe2)" = '__TEXT,__text __DATA,__data __DATA,__zz __FOO,__zerofill ' ]

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o -merge_zero_fill_sections
sects $t/r.o > $t/rsects
grep -q '__DATA,__zerofill __FOO,__zerofill' $t/rsects
not grep -q '__bss' $t/rsects
nm $t/r.o | grep -q ' C _com1$'

# Without the option, a section$start$ symbol makes an empty __zerofill:
# zero-fill, and first of the zero-fill sections, where ld-prime keeps
# a place for it.
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.section __DATA,__ptrs
.p2align 3
.quad section\$start\$__DATA\$__zerofill
EOF2
$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/b.o $t/c.o
[ "$(sects $t/exe3 | grep -o '__DATA,[a-z_]*' | tr '\n' ' ')" = \
  '__DATA,__data __DATA,__ptrs __DATA,__zerofill __DATA,__zfb __DATA,__bss __DATA,__zfa __DATA,__common ' ]
otool -l $t/exe3 | grep -A10 'sectname __zerofill' | grep -q 'flags 0x00000001'
