#!/bin/bash
source "$(dirname "$0")"/common.inc

# -segaddr pins a segment to an address. The others go from the image
# base in segment order, each to the lowest address where it runs into
# no segment placed before it, and the pinned segments count as placed
# only from the first of them on (ld-prime).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
.data
.p2align 3
_hdr: .quad __mh_execute_header
.section __AAA,__a
.quad 1
.space 0x7000
.section __BBB,__b
.quad 2
.section __CCC,__c
.quad 3
EOF

if [ $ARCH = arm64 ]; then page=0x4000; else page=0x1000; fi
addr() { otool -l $1 | awk -v s=$2 '$1 == "segname" && $2 == s { getline; print $2; exit }'; }
size() { otool -l $1 | awk -v s=$2 '$1 == "segname" && $2 == s { getline; getline; print $2; exit }'; }
end() { printf '%#x' $(($(addr $1 $2) + $(size $1 $2))); }
hex() { printf '%#x' $(($1)); }
link() { $mold -arch $ARCH -static -e _main $t/a.o "$@"; }

link -o $t/exe0
text_end=$(end $t/exe0 __TEXT)
aaa=$(addr $t/exe0 __AAA)

# A pin above the others leaves them where they were.
link -o $t/exe1 -segaddr __AAA 0x200000000
[ $(hex $(addr $t/exe1 __AAA)) = 0x200000000 ]
[ $(hex $(addr $t/exe1 __BBB)) = $(end $t/exe1 __DATA) ]
[ $(hex $(addr $t/exe1 __CCC)) = $(end $t/exe1 __BBB) ]
[ $(hex $(addr $t/exe1 __LINKEDIT)) = $(end $t/exe1 __CCC) ]

# The segments after a pin fill the gap below it where they fit.
link -o $t/exe2 -segaddr __DATA $(hex "$text_end + $page")
[ $(hex $(addr $t/exe2 __AAA)) = $(end $t/exe2 __DATA) ]
[ $(hex $(addr $t/exe2 __BBB)) = $text_end ]
[ $(hex $(addr $t/exe2 __CCC)) = $(end $t/exe2 __AAA) ]

# The ones ahead of the first pin do not avoid it.
not link -o $t/exe3 -segaddr __BBB $(hex "$aaa + $page") 2> $t/log3
grep -q "custom segments overlap: __AAA($(hex $aaa)-$(hex "$aaa + 0x8000")) __BBB($(hex "$aaa + $page")-$(hex "$aaa + 2 * $page"))" $t/log3

# With -segment_order, the segments listed after a pinned one follow
# it; the others go back where they were.
link -o $t/exe4 -segment_order __AAA:__BBB -segaddr __AAA 0x200000000 2> /dev/null
[ $(hex $(addr $t/exe4 __BBB)) = 0x200008000 ]
[ $(hex $(addr $t/exe4 __DATA)) = $text_end ]
[ $(hex $(addr $t/exe4 __CCC)) = $(end $t/exe4 __DATA) ]
[ $(hex $(addr $t/exe4 __LINKEDIT)) = $(end $t/exe4 __CCC) ]

# The last -segaddr for a segment wins.
link -o $t/exe5 -segaddr __AAA 0x300000000 -segaddr __AAA 0x200000000 \
  -segaddr __AAA 0x200000000 2> $t/log5
grep -q -- '-segaddr __AAA has conflicting values, using 0x200000000' $t/log5
grep -q -- '-segaddr __AAA used more than once' $t/log5
[ $(hex $(addr $t/exe5 __AAA)) = 0x200000000 ]

# A pin may not lie in __PAGEZERO, be off a page boundary, or share its
# address with another one.
not link -o $t/exe6 -segaddr __AAA 0x80000000 2> $t/log6
grep -q -- '-segaddr __AAA 0x80000000 conflicts with -pagezero_size' $t/log6
not link -o $t/exe6 -segaddr __AAA 0x200000800 2> $t/log6
grep -q -- "-segaddr __AAA 0x200000800 is not aligned to the page size ($page), use -segalign to change it" $t/log6
not link -o $t/exe6 -segaddr __AAA 0x200000000 -segaddr __BBB 0x200000000 2> $t/log6
grep -q -- 'duplicate -segaddr addresses for __AAA and __BBB' $t/log6

# A pinned __TEXT is the image base, and holds the mach header.
link -o $t/exe7 -segaddr __TEXT 0x200000000 -map $t/map7
[ $(hex $(addr $t/exe7 __DATA)) = $(end $t/exe7 __TEXT) ]
nm $t/exe7 | grep -q '^0000000200000000 .*__mh_execute_header'
grep -q '^0x200000000[[:space:]].*__mh_execute_header' $t/map7
off=$(otool -l $t/exe7 | awk '$2 == "__data" { f = 1 } f && $1 == "offset" { print $2; exit }')
[ $(hex 0x$(od -A n -t x8 -j $off -N 8 $t/exe7 | tr -d ' ')) = 0x200000000 ]

link -o $t/exe8 -image_base 0x300000000 -segaddr __TEXT 0x200000000 2> $t/log8
grep -q -- '-image_base and -segaddr __TEXT must match, changing image base to 0x200000000' $t/log8
[ $(hex $(addr $t/exe8 __TEXT)) = 0x200000000 ]
[ $(hex $(addr $t/exe8 __DATA)) = $(end $t/exe8 __TEXT) ]

# ld-prime keeps the segments of an image dyld slides in ascending
# address order, refusing any other, and puts __LINKEDIT above them all.
$CC --ld-path=$mold -o $t/exe9 $t/a.o -Wl,-segaddr,__CCC,0x200000000
[ $(hex $(addr $t/exe9 __LINKEDIT)) = $(end $t/exe9 __CCC) ]
not $CC --ld-path=$mold -o $t/exe10 $t/a.o -Wl,-segaddr,__AAA,0x200000000 2> $t/log10
grep -q 'segment __BBB address is out of order' $t/log10

# A PIE ignores -segaddr __TEXT as its image base: the other segments go
# where they would have, below __TEXT.
not $CC --ld-path=$mold -o $t/exe11 $t/a.o -Wl,-segaddr,__TEXT,0x200000000 2> $t/log11
grep -q 'Linking with PIE, -image_base will be ignored' $t/log11
grep -q 'address is out of order' $t/log11

# A dylib with chained fixups has no preferred address either, but a
# pinned __TEXT stays where it is, and the other segments follow it.
echo 'int x = 1; int f(void) { return x; }' | $CC -o $t/b.o -c -xc -
$CC --ld-path=$mold -shared -o $t/c.dylib $t/b.o -Wl,-segaddr,__TEXT,0x100000 2> $t/log12
grep -q 'prefered load addresses (-seg1addr) are disabled with chained fixups' $t/log12
[ $(hex $(addr $t/c.dylib __TEXT)) = 0x100000 ]
seg=$(otool -l $t/c.dylib | awk '$1 == "segname" && !seen[$2]++ { print $2 }' | sed -n 2p)
[ $(hex $(addr $t/c.dylib $seg)) = $(end $t/c.dylib __TEXT) ]

# A -static image's mach header moves with -rename_segment __TEXT, and
# a -segaddr for its new segment places it; -image_base then only
# places the segments that float.
link -o $t/exe13 -rename_segment __TEXT __FOO -segment_order __FOO:__DATA:__AAA:__BBB:__CCC \
  -segaddr __FOO 0x200000000 -image_base 0x300000000
[ $(hex $(addr $t/exe13 __FOO)) = 0x200000000 ]
[ $(hex $(addr $t/exe13 __DATA)) = $(end $t/exe13 __FOO) ]
[ $(hex $(addr $t/exe13 __LINKEDIT)) = 0x300000000 ]
nm $t/exe13 | grep -q '^0000000200000000 .*__mh_execute_header'
