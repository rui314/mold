#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output's sections lie in the file one after another, each
# aligned as in the address space, a zero-fill one taking no file
# space. The one segment's size is the sections' address span and its
# file size the contents' span, no larger. (ld-prime packs them its own
# way and rounds the segment's size up to 8 bytes.)
for n in 1 3 4; do
  {
    echo '.section __DATA,__data'
    for i in $(seq $n); do echo '.byte 1'; done
    echo '.zerofill __DATA,__bss,_big,400,4'
    echo '.section __ZZZ,__q'
    for i in $(seq $n); do echo '.byte 2'; done
    echo '.section __ZZZ,__r'
    echo '.p2align 3'
    echo '.quad 3'
  } | $CC -o $t/a$n.o -c -xassembler -
  $mold -r -arch $ARCH $t/a$n.o -o $t/r$n.o

  python3 - $t/r$n.o <<'EOF'
import struct, sys
d = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x19:
        vmsize, fileoff, filesize = struct.unpack_from('<QQQ', d, off + 32)
        nsects = struct.unpack_from('<I', d, off + 64)[0]
        sects = [struct.unpack_from('<16s16sQQIIIII', d, off + 72 + i * 80) for i in range(nsects)]
    off += size
end = 0
for name, seg, addr, size, offset, align, _, _, flags in sects:
    if flags & 0xff == 1:
        assert offset == 0, name
        continue
    assert offset % (1 << align) == 0, name
    assert offset >= max(end, fileoff), name
    end = offset + size
assert vmsize == max(s[2] + s[3] for s in sects), vmsize
assert filesize == end - fileoff and filesize <= vmsize, (filesize, vmsize)
EOF

  # The contents are the object's.
  for s in __DATA,__data __ZZZ,__q __ZZZ,__r; do
    otool -s ${s%,*} ${s#*,} $t/a$n.o | tail -n +3 | cut -f2 > $t/in
    otool -s ${s%,*} ${s#*,} $t/r$n.o | tail -n +3 | cut -f2 > $t/out
    [ -s $t/in ]
    diff $t/in $t/out
  done
  nm $t/r$n.o > /dev/null
done
