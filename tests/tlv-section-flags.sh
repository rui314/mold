#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime takes a final image's __thread_data and __thread_bss, in any
# segment and under the names renames give, for the thread-local
# template dyld copies for each thread, and refuses one that its first
# member doesn't type as thread-local data, printing the layout it made
# as it does after such errors in it. A -r output takes it.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__thread int x = 5;
__thread int y;
int main() { printf("%d %d\n", x, y); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__thread_data
.p2align 3
.quad 0
.tbss _z\$tlv\$init, 8, 3
EOF

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __TEXT,__thread_bss
.p2align 3
.quad 0
EOF

# Rewrites the flags of the sections named SEG,SECT in FILE.
set_flags() {
  python3 - "$@" <<'EOF'
import struct, sys
path, seg, sect, flags = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4], 0)
d = bytearray(open(path, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        if d[s:s + 16].rstrip(b'\0') == sect.encode() and d[s + 16:s + 32].rstrip(b'\0') == seg.encode():
            struct.pack_into('<I', d, s + 64, flags)
    off += size
open(path, 'wb').write(d)
EOF
}

cp $t/b.o $t/data.o
set_flags $t/data.o __DATA __thread_data 0
cp $t/b.o $t/bss.o
set_flags $t/bss.o __DATA __thread_bss 1

$CC --ld-path=$mold -o $t/exe $t/a.o $t/data.o
$t/exe | grep -q '^5 0$'
$CC --ld-path=$mold -o $t/exe $t/a.o $t/bss.o
$t/exe | grep -q '^5 0$'

not $CC --ld-path=$mold -o $t/exe2 $t/data.o $t/a.o 2> $t/log
grep -q 'Missing TLV section flags in __DATA,__thread_data' $t/log
grep -q '^final section layout:$' $t/log
# ld-prime finds it before it lays out __LINKEDIT, which it prints
# unsized.
grep -q '^    __LINKEDIT .* size=0x000000000, .*, fileSize=0x00000000$' $t/log

not $CC --ld-path=$mold -o $t/exe2 $t/bss.o $t/a.o 2> $t/log
grep -q 'Missing TLV section flags in __DATA,__thread_bss' $t/log

not $CC --ld-path=$mold -o $t/exe2 $t/data.o $t/a.o -Wl,-rename_segment,__DATA,__FOO 2> $t/log
grep -q 'Missing TLV section flags in __FOO,__thread_data' $t/log

not $CC --ld-path=$mold -o $t/exe2 $t/c.o $t/a.o 2> $t/log
grep -q 'Missing TLV section flags in __TEXT,__thread_bss' $t/log

# ld-prime checks the segments and then their sections in one walk in
# load command order, and stops at the first error: of two sections
# without the flags, the first in the layout, not in the input, and
# not a segment out of order after it. It prints the layout once.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.section __CCC,__thread_data
.quad 0
EOF

not $CC --ld-path=$mold -o $t/exe2 $t/e.o $t/bss.o $t/a.o 2> $t/log
grep -q 'Missing TLV section flags in __DATA,__thread_bss' $t/log
not grep -q '__CCC,__thread_data' $t/log
[ "$(grep -c '^final section layout:$' $t/log)" = 1 ]

cat <<EOF | $CC -o $t/f.o -c -xassembler -
.section __AAA,__a
.quad 0
.space 0x7000
.section __BBB,__b
.quad 0
EOF

not $CC --ld-path=$mold -o $t/exe2 $t/bss.o $t/a.o $t/f.o -Wl,-segaddr,__AAA,0x200000000 \
  2> $t/log
grep -q 'Missing TLV section flags in __DATA,__thread_bss' $t/log
not grep -q 'out of order' $t/log
[ "$(grep -c '^final section layout:$' $t/log)" = 1 ]

# A section past its segment's end in the file, before it in the walk.
not $CC --ld-path=$mold -o $t/exe2 $t/data.o $t/a.o -Wl,-segalign,0x80000000 2> $t/log
grep -q 'file end (2147483656) goes past the segment end (0) $' $t/log
not grep -q 'Missing TLV' $t/log

$mold -arch $ARCH -r -o $t/r.o $t/data.o $t/a.o

# Thread-local data a rename puts in a section its first member types
# otherwise is outside the template, and so is its offset: ld-prime
# finds that of data before the template past 4GB.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.section __DATA,__bar
.long 7
.section __FOO,__bar
.long 7
EOF

not $CC --ld-path=$mold -o $t/exe3 $t/d.o $t/a.o \
  -Wl,-rename_section,__DATA,__thread_data,__DATA,__bar 2> $t/log
grep -q 'thread-locals too large.  Max 4GB for 64-bit architectures$' $t/log
grep -q '^final section layout:$' $t/log
grep -q '^        __bar ' $t/log
not grep -q '^    __LINKEDIT .*fileSize=0x00000000$' $t/log

not $CC --ld-path=$mold -o $t/exe3 $t/d.o $t/a.o \
  -Wl,-rename_section,__DATA,__thread_bss,__DATA,__bar 2> $t/log
grep -q 'thread-locals too large' $t/log

# ld-prime finds it after the errors of its walk over the segments.
not $CC --ld-path=$mold -o $t/exe3 $t/d.o $t/a.o $t/f.o \
  -Wl,-rename_section,__DATA,__thread_data,__DATA,__bar -Wl,-segaddr,__AAA,0x200000000 2> $t/log
grep -q 'segment __BBB address is out of order' $t/log
not grep -q 'thread-locals' $t/log

$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/d.o \
  -Wl,-rename_section,__DATA,__thread_data,__DATA,__bar
$t/exe3 | grep -q '^5 0$'

# ld-prime writes an image dyld refuses for data after the template,
# and crashes with no template left.
if $mold -v 2>&1 | grep -q mold-macho; then
  not $CC --ld-path=$mold -o $t/exe3 $t/d.o $t/a.o \
    -Wl,-rename_section,__DATA,__thread_data,__FOO,__bar 2> $t/log
  grep -q 'thread-locals too large' $t/log

  not $CC --ld-path=$mold -o $t/exe3 $t/d.o $t/a.o \
    -Wl,-rename_section,__DATA,__thread_data,__DATA,__bar \
    -Wl,-rename_section,__DATA,__thread_bss,__DATA,__bar 2> $t/log
  grep -q 'thread-locals too large' $t/log
fi

# dyld copies the template as one block: ld-prime refuses sections of it
# that a rename or a symbol move parted, naming the first two with
# another section between them in the layout, even across segments. It
# finds that in its walk too, before a segment out of order after them,
# and before data outside the template.
cat <<EOF2 | $CC -o $t/h.o -c -xassembler -
.zerofill __DATA,__bss,_h,8,3
EOF2

$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-rename_section,__DATA,__thread_data,__FOO,__thread_data
not $CC --ld-path=$mold -o $t/exe4 $t/a.o $t/h.o \
  -Wl,-rename_section,__DATA,__thread_data,__FOO,__thread_data 2> $t/log
grep -q 'TLV sections must be contiguous, but __DATA,__thread_bss - __FOO,__thread_data aren.t$' \
  $t/log
[ "$(grep -c '^final section layout:$' $t/log)" = 1 ]
grep -q '^    __LINKEDIT .* size=0x000000000, .*, fileSize=0x00000000$' $t/log

echo '_x$tlv$init' > $t/list
not $CC --ld-path=$mold -o $t/exe4 $t/a.o $t/h.o -Wl,-move_to_rw_segment,__FOO,$t/list 2> $t/log
grep -q 'TLV sections must be contiguous, but __DATA,__thread_bss - __FOO,__thread_data' $t/log

not $CC --ld-path=$mold -o $t/exe4 $t/a.o $t/h.o $t/f.o -Wl,-segaddr,__AAA,0x200000000 \
  -Wl,-rename_section,__DATA,__thread_data,__FOO,__thread_data 2> $t/log
grep -q 'TLV sections must be contiguous' $t/log
not grep -q 'out of order' $t/log
