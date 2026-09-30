#!/bin/bash
source "$(dirname "$0")"/common.inc

# A section only a section$start$ or section$end$ symbol makes gets the
# flags a compiler marks a section of the name the symbol gives with,
# or ld-prime its own section of that name, as in ld-prime - also when
# the section moves to __DATA_CONST - and none for another name. That
# holds in the segments the linker consumes too: __LD,__compact_unwind
# has the debug attribute clang gives it, the __DWARF sections none.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
.data
.p2align 3
.quad section\$start\$__TEXT\$__literal8
.quad section\$start\$__TEXT\$__StaticInit
.quad section\$start\$__TEXT\$__cstring
.quad section\$start\$__TEXT\$__oslogstring
.quad section\$start\$__TEXT\$__objc_clsstubs
.quad section\$start\$__DATA\$__la_resolver
.quad section\$start\$__DATA\$__got
.quad section\$start\$__DATA\$__objc_selrefs
.quad section\$start\$__DATA\$__objc_classlist
.quad section\$start\$__DATA\$__thread_vars
.quad section\$start\$__DATA\$__bss
.quad section\$start\$__DATA\$__foo
.quad section\$start\$__LD\$__compact_unwind
.quad section\$start\$__DWARF\$__debug_info
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
flags() {
  otool -l $t/exe | awk -v g=$1 -v s=$2 '$1 == "sectname" { n = $2 }
    $1 == "segname" { m = $2 } n == s && m == g && $1 == "flags" { print $2; exit }'
}
[ "$(flags __TEXT __literal8)" = 0x00000004 ]
[ "$(flags __TEXT __StaticInit)" = 0x80000400 ]
[ "$(flags __TEXT __cstring)" = 0x00000002 ]
[ "$(flags __TEXT __oslogstring)" = 0x00000002 ]
[ "$(flags __TEXT __objc_clsstubs)" = 0x80000400 ]
[ "$(flags __DATA __la_resolver)" = 0x00000007 ]
[ "$(flags __DATA_CONST __got)" = 0x00000006 ]
[ "$(flags __DATA __objc_selrefs)" = 0x10000005 ]
[ "$(flags __DATA_CONST __objc_classlist)" = 0x10000000 ]
[ "$(flags __DATA __thread_vars)" = 0x00000013 ]
[ "$(flags __DATA __bss)" = 0x00000001 ]
[ "$(flags __DATA __foo)" = 0x00000000 ]
[ "$(flags __LD __compact_unwind)" = 0x02000000 ]
[ "$(flags __DWARF __debug_info)" = 0x00000000 ]
