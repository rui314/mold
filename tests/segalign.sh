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
  # It builds the segment's chain record all the same, and its
  # __LINKEDIT is as large as with 16KB pages.
  not $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segalign,0x8000,-no_adhoc_codesign 2> $t/log
  $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segalign,0x4000,-no_adhoc_codesign
  size=$(otool -l $t/exe4 | grep -A8 'segname __LINKEDIT' | awk '$1 == "filesize" { print $2 }')
  grep -q "^    __LINKEDIT .*fileSize=$(printf '0x%08x' $size)$" $t/log
  $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segalign,0x8000,-no_fixup_chains
  # In a page over 16KB, two fixups may be too far apart to chain, which
  # ld-prime finds first.
  cat <<EOF | $CC -o $t/far.o -c -xc -
int x = 5;
int *p = &x;
char gap[20000] = {1};
int *q = &x;
int main() { return *p + *q - 10; }
EOF
  not $CC --ld-path=$mold -o $t/exe7 $t/far.o -Wl,-segalign,0x8000 2> $t/log
  grep -q 'distance between fixups (20008) is not encodable in chain for fixup at __DATA+0x8, $' $t/log
  not grep -q page_size $t/log
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
grep -q -- -segalign $t/log

# ld-prime keeps a section's file offset in 32 bits, and so its
# segment's end: a segment that ends at 4 GiB ends at 0, before the
# section. It prints the layout twice, and a -r output takes either.
not $CC --ld-path=$mold -o $t/exe7 $t/a.o -Wl,-segalign,0x80000000 2> $t/log
grep -q 'section __DATA,__data file end (2147483664) goes past the segment end (0) $' $t/log
[ "$(grep -c '^final section layout:$' $t/log)" = 2 ]
grep -q '^    __LINKEDIT  *addr=0x200000000, size=0x000000000, fileOffset=0x00000000, fileSize=0x00000000$' $t/log

# A -segalign of 0, pages of no size, is refused. (ld-prime takes it,
# for segments of no size, which fails a final link only.)
not $CC --ld-path=$mold -o $t/exe8 $t/b.o -Wl,-segalign,0 2> $t/log
grep -q -- -segalign $t/log
if $mold -v 2> /dev/null | grep -q mold-macho; then
  not $mold -arch $ARCH -r -o $t/r.o $t/a.o -segalign 0
fi
$mold -arch $ARCH -r -o $t/r.o $t/a.o -segalign 0x80000000

# The linker's own sections are reduced as well, in output order. A
# pointer 8-aligned in its segment is fine, though the segment is off 8
# bytes: ld-prime gives x86-64's fixup chains up without a word of it,
# and fails an arm64 link on the chain pages, printing the layout.
cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>
int x = 5;
int main() { printf("%d\n", x); }
EOF
if [ $ARCH = arm64 ]; then
  not $CC --ld-path=$mold -o $t/exe9 $t/d.o -Wl,-segalign,0x1 2> $t/log
  grep -q 'chained fixups, page_size not 4KB or 16KB in segment #2' $t/log
  grep -q '^final section layout:$' $t/log
else
  $CC --ld-path=$mold -o $t/exe9 $t/d.o -Wl,-segalign,0x1 2> $t/log
  grep -q 'disabling chained fixups because of unaligned pointers' $t/log
fi
not grep -q 'pointer not aligned' $t/log
grep -o 'reducing alignment of section [^ ]*' $t/log | cut -d' ' -f5 | tr '\n' ' ' > $t/sects
[ "$(cat $t/sects)" = '__TEXT,__text __TEXT,__stubs __TEXT,__unwind_info __DATA_CONST,__got __DATA,__data ' ]

# ld-prime writes the chains after it has applied the relocations: one
# it can't apply, as a load from an address the reduced alignment left
# unaligned, fails the link first.
cat <<EOF | $CC -o $t/e.o -c -xc -
#include <stdio.h>
#include <stdlib.h>
int x = 5;
int *p = &x;
__thread int tv = 3;
int main() { printf("%d %d\n", x, tv); exit(*p); }
EOF
if [ $ARCH = arm64 ]; then
  not $CC --ld-path=$mold -o $t/exe10 $t/e.o -Wl,-segalign,0x1 2> $t/log
  grep -q "fixup error (kind=arm64_lo12) at '_main'+0x30 from e.o, target '_x' not 4-byte aligned" $t/log
  not grep -q 'page_size not 4KB' $t/log
fi
