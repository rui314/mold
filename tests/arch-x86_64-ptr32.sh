#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = x86_64 ] || skip

# A lone 4-byte UNSIGNED (.long sym) is a 32-bit pointer, which dyld
# can neither slide nor bind. ld-prime rejects one in any image dyld or
# kmutil loads, whatever it points to, and takes it in a -static or
# -preload image only where the address fits in 32 bits.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.data
.globl _g
.p2align 3
_g: .quad 0
.long _ext
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/ext.o -c -xc -
int ext = 42;
EOF
$CC --ld-path=$mold -shared -o $t/libext.dylib $t/ext.o

not $CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o $t/ext.o 2> $t/log
grep -qF "32-bit pointer used in 64-bit code in '_g'+0x8 (" $t/log
grep -qF "/a.o)" $t/log

not $CC --ld-path=$mold -shared -o $t/c.dylib $t/a.o $t/libext.dylib 2> $t/log
grep -qF "32-bit pointer used in 64-bit code in '_g'+0x8 (" $t/log

not $mold -arch x86_64 -kext -o $t/d.kext $t/a.o 2> $t/log
grep -qF "32-bit pointer used in 64-bit code in '_g'+0x8 (" $t/log

# Below 4 GiB the pointer holds the address.
$mold -arch x86_64 -static -e _g -pagezero_size 0 -image_base 0x1000 -o $t/e $t/a.o $t/ext.o
nm $t/e > $t/nm
otool -s __DATA __data $t/e > $t/data
ext=$(awk '$3 == "_ext" { print substr($1, 13, 4) }' $t/nm)
grep -q "00 00 00 00 00 00 00 00 ${ext:2:2} ${ext:0:2} 00 00" $t/data

$mold -arch x86_64 -preload -e _g -o $t/f $t/a.o $t/ext.o

# Above it (after the default 4 GiB __PAGEZERO) it overflows.
not $mold -arch x86_64 -static -e _g -o $t/g $t/a.o $t/ext.o 2> $t/log
grep -qF "$t/a.o: _g+0x8: 32-bit pointer overflow" $t/log

# One where any pointer would be a text relocation is listed as one;
# every other fails the link, as do the text relocations. (ld-prime
# reports only one 32-bit pointer - with chained fixups the last
# section's, in place of the text relocations; with classic dyld info
# none if there are text relocations, else the first.)
cat <<EOF2 | $CC -o $t/h.o -c -xassembler -
.section __TEXT,__const
.p2align 3
.globl _tc
_tc: .long _ext
.data
.globl _d1, _d2
_d1: .long _ext
.long _ext
_d2: .long _ext
.section __DATA,__foo
.globl _f1, _f2
_f1: .long _ext
.long _ext
_f2: .long _ext
.subsections_via_symbols
EOF2
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -
not $CC --ld-path=$mold -o $t/h $t/main.o $t/h.o $t/ext.o 2> $t/log
grep -q "text-relocation in '_tc' (.*/h.o) to '_ext'" $t/log
grep -q "32-bit pointer used in 64-bit code in '_f1'+0x4 (.*/h.o)" $t/log
[ "$(grep -c '32-bit pointer' $t/log)" = 6 ]
grep -q 'Found illegal text-relocations' $t/log

not $CC --ld-path=$mold -o $t/h $t/main.o $t/h.o $t/ext.o -Wl,-no_fixup_chains 2> $t/log
grep -q "text-relocation in '_tc' (.*/h.o) to '_ext'" $t/log
grep -q 'Found illegal text-relocations' $t/log
[ "$(grep -c '32-bit pointer' $t/log)" = 6 ]

not $CC --ld-path=$mold -o $t/h $t/main.o $t/a.o $t/ext.o -Wl,-no_fixup_chains 2> $t/log
grep -q "32-bit pointer used in 64-bit code in '_g'+0x8 (" $t/log
