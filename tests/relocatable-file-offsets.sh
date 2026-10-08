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
import sys, macho
m = macho.MachO(sys.argv[1])
seg, = m.segments
end = 0
for s in m.sections:
    if s.flags & 0xff == 1:  # S_ZEROFILL
        assert s.offset == 0, s.sectname
        continue
    assert s.offset % (1 << s.align) == 0, s.sectname
    assert s.offset >= max(end, seg.fileoff), s.sectname
    end = s.offset + s.size
assert seg.vmsize == max(s.addr + s.size for s in m.sections), seg.vmsize
assert seg.filesize == end - seg.fileoff and seg.filesize <= seg.vmsize, (seg.filesize, seg.vmsize)
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
