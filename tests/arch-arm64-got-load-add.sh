#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = arm64 ] || skip

# The offset half of a GOT load of a symbol defined in the image
# relaxes: ld-prime turns the ldr of the slot into an add of the
# symbol's page offset, takes a 64-bit add as one already, and keeps no
# GOT slot, but refuses any other instruction. A load is one fixup with
# the adrp before it that sets its base register, which the error then
# names. A TLV load relaxes an ldr of either width.

# Rewrites the instruction under the first relocation of type TYPE in
# __text to (insn & AND) | OR.
cat > $t/insn.py <<'EOF2'
import struct, sys
src, dst, rtype = sys.argv[1], sys.argv[2], int(sys.argv[3])
mask, bits = int(sys.argv[4], 0), int(sys.argv[5], 0)
d = bytearray(open(src, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        if d[s:s + 16].rstrip(b'\0') == b'__text':
            base, _, reloff, nreloc = struct.unpack_from('<IIII', d, s + 48)
            addrs = [struct.unpack_from('<I', d, reloff + 8 * j)[0] for j in range(nreloc)
                     if struct.unpack_from('<I', d, reloff + 8 * j + 4)[0] >> 28 == rtype]
            p = base + min(addrs)
            insn = struct.unpack_from('<I', d, p)[0]
            struct.pack_into('<I', d, p, insn & mask | bits)
    off += size
open(dst, 'wb').write(d)
EOF2

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  adrp x0, _val@GOTPAGE
  ldr x0, [x0, _val@GOTPAGEOFF]
  ldr w0, [x0]
  ret
.data
.globl _val
.p2align 2
_val: .long 42
.subsections_via_symbols
EOF

# add x0, x0: the address of _val itself, with no GOT.
python3 $t/insn.py $t/a.o $t/add.o 6 0x3ff 0x91000000
$CC --ld-path=$mold -o $t/exe $t/add.o
code=0
$t/exe || code=$?
[ $code = 42 ]
otool -l $t/exe > $t/lc
not grep -q 'sectname __got' $t/lc

# add w0, w0: refused, at the add.
python3 $t/insn.py $t/a.o $t/addw.o 6 0x3ff 0x11000000
not $CC --ld-path=$mold -o $t/exe2 $t/addw.o 2> $t/log
grep -qF "fixup error (kind=arm64_was_ld12_got_elide_got) at '_main'+0x4 from addw.o, non-LDR instruction" $t/log

# ldr d0, [x0]: refused, at the adrp it pairs with.
python3 $t/insn.py $t/a.o $t/ldrd.o 6 0x3ff 0xfd400000
not $CC --ld-path=$mold -o $t/exe3 $t/ldrd.o 2> $t/log
grep -qF "fixup error (kind=arm64_was_adrp_ldr_got_elide_got) at '_main' from ldrd.o, non-LDR instruction" $t/log

# The same of a dylib's symbol keeps the GOT slot: the add takes its
# address.
cat <<EOF | $CC -o $t/b.o -c -xc -
int val = 42;
EOF
$CC --ld-path=$mold -shared -o $t/libb.dylib $t/b.o
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _get
.p2align 2
_get:
  adrp x0, _val@GOTPAGE
  ldr x0, [x0, _val@GOTPAGEOFF]
  ret
.subsections_via_symbols
EOF
python3 $t/insn.py $t/c.o $t/c-addw.o 6 0x3ff 0x11000000
$CC --ld-path=$mold -shared -o $t/libc.dylib $t/c-addw.o $t/libb.dylib

# A TLV load: an ldr of a W register relaxes to a 64-bit add; an ldrb
# is refused.
cat <<EOF | $CC -o $t/d.o -c -xc -
_Thread_local int tv = 7;
int main() { return tv; }
EOF
python3 $t/insn.py $t/d.o $t/ldrw.o 9 0xbfffffff 0
$CC --ld-path=$mold -o $t/exe4 $t/ldrw.o
code=0
$t/exe4 || code=$?
[ $code = 7 ]
python3 $t/insn.py $t/d.o $t/ldrb.o 9 0x3fffff 0x39400000
not $CC --ld-path=$mold -o $t/exe5 $t/ldrb.o 2> $t/log
grep -qF "fixup error (kind=arm64_was_ld12_tlv_elide_got) at '_main'" $t/log
grep -qF "from ldrb.o, non-LDR instruction" $t/log
