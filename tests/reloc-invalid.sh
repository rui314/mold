#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime checks each relocation record as it reads an object - in
# -r, and in an archive member the link doesn't use, too - and rejects
# the object at the first one it doesn't take: a type the target
# doesn't define, pcrel, length or extern bits the type doesn't take, a
# SUBTRACTOR or ADDEND without its partner, a field that runs out of
# its atom, a symbol or section index out of range, and on arm64 an
# instruction the type can't patch, or one that embeds an addend.

# Rewrites relocation record IDX (in table order; an assembler lists a
# section's from the last to the first) of section SECT in the copy OUT
# of IN. KEY=VALUE sets its type, pcrel, length, extern, sym or addr;
# insn writes a 32-bit word where it points and opcode the byte before.
cat > $t/patch.py <<'EOF2'
import struct, sys
src, dst, sect, idx = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
d = bytearray(open(src, 'rb').read())
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    for i in range(struct.unpack_from('<I', d, off + 64)[0] if cmd == 0x19 else 0):
        s = off + 72 + i * 80
        if d[s:s + 16].rstrip(b'\0').decode() == sect:
            base = struct.unpack_from('<I', d, s + 48)[0]
            p = struct.unpack_from('<I', d, s + 56)[0] + idx * 8
    off += size
addr, w = struct.unpack_from('<iI', d, p)
f = dict(sym=w & 0xffffff, pcrel=w >> 24 & 1, length=w >> 25 & 3,
         extern=w >> 27 & 1, type=w >> 28, addr=addr)
for k, v in (a.split('=') for a in sys.argv[5:]):
    if k == 'insn':
        struct.pack_into('<I', d, base + addr, int(v, 0))
    elif k == 'opcode':
        d[base + addr - 1] = int(v, 0)
    else:
        f[k] = int(v, 0)
w = f['sym'] | f['pcrel'] << 24 | f['length'] << 25 | f['extern'] << 27 | f['type'] << 28
struct.pack_into('<iI', d, p, f['addr'], w)
open(dst, 'wb').write(d)
EOF2
patch_reloc() { python3 $t/patch.py "$@"; }

cat <<EOF | $CC -o $t/main.o -c -xc -
int main() { return 0; }
EOF
cat <<EOF | $CC -o $t/ext.o -c -xc -
int ext = 42;
EOF

# Fails -r and a final link of OBJ with a message ending in MSG. (A
# function doesn't inherit the ERR trap, so its steps are chained.)
check() {
  not $mold -r -arch $ARCH -o $t/r.o $1 2> $t/log &&
    grep -qF "$2" $t/log &&
    not $CC --ld-path=$mold -o $t/exe $t/main.o $1 $t/ext.o 2> $t/log &&
    grep -qF "$2" $t/log
}

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _g
_g: ret
.data
.globl _d
.p2align 3
_d: .quad 0
_e: .quad _ext
_e2: .quad _g - _d
.subsections_via_symbols
EOF

# Records 0 and 1 are the SUBTRACTOR pair at 0x10, record 2 the
# UNSIGNED at 0x8.
patch_reloc $t/a.o $t/a1.o __data 2 type=15
check $t/a1.o "relocation in '_e' is not supported: r_address=0x8, r_type=15, r_extern=1, r_pcrel=0, r_length=3 in '$t/a1.o'"

patch_reloc $t/a.o $t/a2.o __data 2 pcrel=1
check $t/a2.o "relocation in '_e' is not supported: r_address=0x8, r_type=0, r_extern=1, r_pcrel=1, r_length=3 in '$t/a2.o'"

patch_reloc $t/a.o $t/a3.o __data 2 sym=1000
check $t/a3.o "r_symbolnum=1000 out of range in '$t/a3.o'"

patch_reloc $t/a.o $t/a4.o __data 2 extern=0 sym=9
check $t/a4.o "sectionNum=9 out of range (size="

patch_reloc $t/a.o $t/a5.o __data 2 addr=0x4
check $t/a5.o "8 byte relocaton at r_address (0x0004) is not fully within bounds of atom 0x0000->0x0008 in '$t/a5.o'"

patch_reloc $t/a.o $t/a6.o __data 0 addr=0x14
check $t/a6.o "8 byte relocaton at r_address (0x0014) is not fully within bounds of atom 0x0010->0x0018 in '$t/a6.o'"

[ $ARCH = arm64 ] && prefix=ARM64 || prefix=X86_64
patch_reloc $t/a.o $t/a7.o __data 1 addr=0x8
check $t/a7.o "${prefix}_RELOC_SUBTRACTOR preceeding ${prefix}_RELOC_UNSIGNED must have same r_address: r_address=0x8, r_type=0, r_extern=1, r_pcrel=0, r_length=3 in '$t/a7.o'"

patch_reloc $t/a.o $t/a8.o __data 1 length=2
check $t/a8.o "relocation in '_e2' is not supported: r_address=0x10, r_type="

# __LD,__compact_unwind's relocations are checked the same way, each
# 32-byte record being an atom. (mold used to index the symbol table
# with an out-of-range r_symbolnum there.)
cat <<EOF | $CC -o $t/u.o -c -xc -
void f() {}
EOF
patch_reloc $t/u.o $t/u1.o __compact_unwind 0 type=15
not $mold -r -arch $ARCH -o $t/r.o $t/u1.o 2> $t/log
grep -qF "is not supported: r_address=0x0, r_type=15, r_extern=0, r_pcrel=0, r_length=3 in '$t/u1.o'" $t/log
patch_reloc $t/u.o $t/u2.o __compact_unwind 0 extern=1 sym=1000
check $t/u2.o "r_symbolnum=1000 out of range in '$t/u2.o'"

# Only an object's first bad record is reported, but every object's is,
# an archive member the link doesn't use included.
patch_reloc $t/a1.o $t/a9.o __data 0 type=14
rm -f $t/lib.a
ar rcs $t/lib.a $t/a1.o
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a9.o $t/ext.o $t/lib.a 2> $t/log
grep -qF "r_type=14, r_extern=1, r_pcrel=0, r_length=3 in '$t/a9.o'" $t/log
not grep -qF "r_type=15, r_extern=1, r_pcrel=0, r_length=3 in '$t/a9.o'" $t/log
grep -qF "r_type=15, r_extern=1, r_pcrel=0, r_length=3 in '$t/lib.a(a1.o)'" $t/log

if [ $ARCH = arm64 ]; then
  # `.quad _x@GOT` is a POINTER_TO_GOT, which ld-prime takes only as a
  # 4-byte pcrel field; a final link used to panic on it.
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.globl _q1
.p2align 3
_q1: .quad 0
_q2: .quad _ext@GOT
.subsections_via_symbols
EOF
  check $t/b.o "relocation in '_q2' is not supported: r_address=0x8, r_type=7, r_extern=1, r_pcrel=0, r_length=3 in '$t/b.o'"
  not $CC --ld-path=$mold -shared -o $t/b.dylib $t/b.o $t/ext.o 2> $t/log
  grep -qF "relocation in '_q2' is not supported" $t/log

  # A lone 4-byte UNSIGNED would be a 32-bit pointer.
  cat <<EOF | $CC -o $t/c.o -c -xassembler -
.data
.globl _p
.p2align 2
_p: .long _ext
.subsections_via_symbols
EOF
  check $t/c.o "32-bit pointer in 64-bit arch: r_address=0x0, r_type=0, r_extern=1, r_pcrel=0, r_length=2 in '$t/c.o'"

  # Record 0 is the BRANCH26 at 0x8, 1 the GOT_LOAD_PAGEOFF12 at 0x4
  # and 2 the GOT_LOAD_PAGE21 at 0x0.
  cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
.globl _f
.p2align 2
_f:
  adrp x0, _ext@GOTPAGE
  ldr x0, [x0, _ext@GOTPAGEOFF]
  bl _g
  ret
.globl _g
_g:
  ret
.subsections_via_symbols
EOF
  patch_reloc $t/d.o $t/d1.o __text 0 type=3
  check $t/d1.o "ARM64_RELOC_PAGE21 relocation on non-ADRP instruction: r_address=0x8, r_type=3, r_extern=1, r_pcrel=1, r_length=2 in '$t/d1.o'"

  patch_reloc $t/d.o $t/d2.o __text 2 type=2
  check $t/d2.o "ARM64_RELOC_BRANCH26 relocation on non-b/bl instruction: r_address=0x0, r_type=2, r_extern=1, r_pcrel=1, r_length=2 in '$t/d2.o'"

  patch_reloc $t/d.o $t/d3.o __text 0 insn=0x94000001
  check $t/d3.o "B/BL has embedded addend. ARM64_RELOC_ADDEND should be used instead: r_address=0x8, r_type=2, r_extern=1, r_pcrel=1, r_length=2 in '$t/d3.o'"

  patch_reloc $t/d.o $t/d4.o __text 1 insn=0xb9400000
  check $t/d4.o "ARM64_RELOC_GOT_LOAD_PAGEOFF12 on LDR that is not an 8-byte load: r_address=0x4, r_type=6, r_extern=1, r_pcrel=0, r_length=2 in '$t/d4.o'"

  patch_reloc $t/d.o $t/d5.o __text 1 type=9 insn=0x91000000
  check $t/d5.o "ARM64_RELOC_TLVP_LOAD_PAGEOFF12 relocation on non-LDR instruction: r_address=0x4, r_type=9, r_extern=1, r_pcrel=0, r_length=2 in '$t/d5.o'"

  # Only an UNSIGNED may be section-relative.
  patch_reloc $t/d.o $t/d6.o __text 0 extern=0 sym=1
  check $t/d6.o "relocation in '_f' is not supported: r_address=0x8, r_type=2, r_extern=0, r_pcrel=1, r_length=2 in '$t/d6.o'"
else
  # Record 0 is the GOT_LOAD at 0x3.
  cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
.globl _f
_f:
  movq _ext@GOTPCREL(%rip), %rax
  ret
.subsections_via_symbols
EOF
  patch_reloc $t/d.o $t/d1.o __text 0 pcrel=0
  check $t/d1.o "relocation in '_f' is not supported: r_address=0x3, r_type=3, r_extern=1, r_pcrel=0, r_length=2 in '$t/d1.o'"

  # A GOT or TLV reference must name a symbol.
  patch_reloc $t/d.o $t/d2.o __text 0 extern=0 sym=1
  check $t/d2.o "relocation in '_f' is not supported: r_address=0x3, r_type=3, r_extern=0, r_pcrel=1, r_length=2 in '$t/d2.o'"

  # A one-byte branch (jmp rel8) to a symbol is fine. The jmp becomes
  # one, with three nops after it.
  cat <<EOF | $CC -o $t/e.o -c -xassembler -
.text
.globl _main
_main:
  movl \$3, %eax
  jmp _g
  movl \$100, %eax
.globl _g
_g:
  addl \$4, %eax
  ret
.subsections_via_symbols
EOF
  patch_reloc $t/e.o $t/e1.o __text 0 length=0 opcode=0xeb insn=0x90909000
  $CC --ld-path=$mold -o $t/exe $t/e1.o
  code=0
  $t/exe || code=$?
  [ $code = 7 ]
  $mold -r -arch $ARCH -o $t/r.o $t/e1.o
  otool -rv $t/r.o > $t/relocs
  grep -q 'True *byte *True *BRANCH' $t/relocs

  # But it can't reach a symbol more than 127 bytes away, or one in a
  # dylib.
  cat <<EOF | $CC -o $t/f.o -c -xassembler -
.text
.globl _main
_main:
  movl \$3, %eax
  jmp _g
  .space 303, 0x90
.globl _g
_g:
  ret
.subsections_via_symbols
EOF
  patch_reloc $t/f.o $t/f1.o __text 0 length=0 opcode=0xeb insn=0x90909000
  not $CC --ld-path=$mold -o $t/exe $t/f1.o 2> $t/log
  grep -Eq "fixup error \(kind=x86_64_branch8\) at '_main'\+0x6 from f1.o, 8-bit branch out of range \(displacement=306, max is \+/-127\), from 0x[0-9A-F]+ to 0x[0-9A-F]+ \('_g'\)" $t/log

  $CC --ld-path=$mold -shared -o $t/libext.dylib $t/ext.o
  cat <<EOF | $CC -o $t/g.o -c -xassembler -
.text
.globl _main
_main:
  jmp _ext
  ret
.subsections_via_symbols
EOF
  patch_reloc $t/g.o $t/g1.o __text 0 length=0 opcode=0xeb insn=0x90909000
  not $CC --ld-path=$mold -o $t/exe $t/g1.o $t/libext.dylib 2> $t/log
  grep -qF "fixup error (kind=x86_64_branch8) at '_main'+0x1 from g1.o, target '_ext' does not have address" $t/log
fi
