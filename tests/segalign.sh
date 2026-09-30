#!/bin/bash
source "$(dirname "$0")"/common.inc

# -segalign sets the boundary segments start and end on, in memory and
# in the file, in place of the page size. ld-prime rounds it down to a
# power of two, and cuts an arm64 image's fixup chains in pages of it,
# which dyld takes only 4 KiB or 16 KiB long; an x86-64 image's are cut
# in 4 KiB pages whatever it is.
cat <<EOF | $CC -o $t/a.o -c -xc -
int x = 5;
int *p = &x;
int main() { return *p - 5; }
EOF
echo 'int main() { return 0; }' | $CC -o $t/b.o -c -xc -

vmsize() {
  otool -l $1 | grep -A3 "segname $2" | awk '$1 == "vmsize" { print $2 }'
}

$CC --ld-path=$mold -o $t/exe1 $t/b.o -Wl,-segalign,0x8000
[ "$(vmsize $t/exe1 __TEXT)" = 0x0000000000008000 ]
[ "$(vmsize $t/exe1 __LINKEDIT)" = 0x0000000000008000 ]

$CC --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-segalign,0x3000 2> $t/log
grep -q 'alignment for -segalign 0x3000 is not a power of two, using 0x2000' $t/log
[ "$(vmsize $t/exe2 __TEXT)" = 0x0000000000002000 ]

$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-segalign,0x1000
[ "$(vmsize $t/exe3 __DATA)" = 0x0000000000001000 ]

if [ $ARCH = arm64 ]; then
  not $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segalign,0x8000 2> $t/log
  grep -q 'chained fixups, page_size not 4KB or 16KB in segment #2' $t/log
  $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segalign,0x8000,-no_fixup_chains
else
  $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segalign,0x8000
  if native_arch; then
    $t/exe4
  fi
fi
[ "$(vmsize $t/exe4 __DATA)" = 0x0000000000008000 ]

# A section may be aligned no more than its segment.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __TEXT,__foo
.p2align 13
.globl _foo
_foo: .quad 0
EOF
$CC --ld-path=$mold -o $t/exe5 $t/b.o $t/c.o -Wl,-segalign,0x1000 2> $t/log
grep -q 'reducing alignment of section __TEXT,__foo from 0x2000 to 0x1000' $t/log

not $CC --ld-path=$mold -o $t/exe6 $t/b.o -Wl,-segalign,0x100000000 2> $t/log
grep -q -- '-segalign 4294967296: alignemnt too big' $t/log

# ld-prime keeps a section's file offset in 32 bits, and so its
# segment's end: a segment that ends at 4 GiB ends at 0, before the
# section. With a -segalign of 0, pages have no size, nor then do
# segments. It prints the layout twice, and a -r output takes either.
not $CC --ld-path=$mold -o $t/exe7 $t/a.o -Wl,-segalign,0x80000000 2> $t/log
grep -q 'section __DATA,__data file end (2147483664) goes past the segment end (0) $' $t/log
[ "$(grep -c '^final section layout:$' $t/log)" = 2 ]
grep -q '^    __LINKEDIT  *addr=0x200000000, size=0x000000000, fileOffset=0x00000000, fileSize=0x00000000$' $t/log

not $CC --ld-path=$mold -o $t/exe8 $t/b.o -Wl,-segalign,0 2> $t/log
grep -q -- '-pagezero_size not aligned, rounded up to: 0, use -segalign to change the alignment' $t/log
grep -q 'reducing alignment of section __TEXT,__text from 0x[0-9a-f]* to 0x1 because' $t/log
grep -Eq 'section __TEXT,__text file end \([0-9]+\) goes past the segment end \(0\) $' $t/log
grep -q '^    __TEXT  *addr=0x000000000, size=0x000000000, fileOffset=0x00000000, fileSize=0x00000000$' $t/log

$mold -arch $ARCH -r -o $t/r.o $t/a.o -segalign 0
$mold -arch $ARCH -r -o $t/r.o $t/a.o -segalign 0x80000000
