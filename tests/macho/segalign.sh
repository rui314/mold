#!/bin/bash
source "$(dirname "$0")"/common.inc

# -segalign sets the boundary segments start and end on, in memory and
# in the file, in place of the page size. ld-prime rounds it down to a
# power of two, and cuts an arm64 image's fixup chains in pages of it,
# which dyld takes only 4 KiB or 16 KiB long: an arm64 image with
# chained fixups and another -segalign fails. An x86-64 image's chains
# are cut in 4 KiB pages whatever it is.
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
  grep -q 'chained fixups' $t/log
  $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segalign,0x8000,-no_fixup_chains
  $RUN $t/exe4
else
  $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-segalign,0x8000
  if native_arch; then
    $RUN $t/exe4
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
# section. A -r output takes either.
not $CC --ld-path=$mold -o $t/exe7 $t/a.o -Wl,-segalign,0x80000000 2> $t/log
grep -q 'section __DATA,__data file end (2147483664) goes past the segment end (0) $' $t/log

# A -segalign of 0, pages of no size, is refused. (ld-prime takes it,
# for segments of no size, which fails a final link only.)
not $CC --ld-path=$mold -o $t/exe8 $t/b.o -Wl,-segalign,0 2> $t/log
grep -q -- -segalign $t/log
if is_mold; then
  not $mold -arch $ARCH -r -o $t/r.o $t/a.o -segalign 0
fi
$mold -arch $ARCH -r -o $t/r.o $t/a.o -segalign 0x80000000

# The linker's own sections are reduced as well, in output order. A GOT
# slot the reduced alignment leaves unaligned makes x86-64 give its
# fixup chains up; an arm64 link fails on the chain pages.
cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>
int x = 5;
int main() { printf("%d\n", x); }
EOF
if [ $ARCH = arm64 ]; then
  not $CC --ld-path=$mold -o $t/exe9 $t/d.o -Wl,-segalign,0x1 2> $t/log
  grep -q 'chained fixups' $t/log
else
  $CC --ld-path=$mold -o $t/exe9 $t/d.o -Wl,-segalign,0x1 2> $t/log
  grep -q 'disabling chained fixups because of unaligned pointers' $t/log
fi
not grep -q 'pointer not aligned' $t/log
grep -o 'reducing alignment of section [^ ]*' $t/log | cut -d' ' -f5 | tr '\n' ' ' > $t/sects
[ "$(cat $t/sects)" = '__TEXT,__text __TEXT,__stubs __TEXT,__unwind_info __DATA_CONST,__got __DATA,__data ' ]
