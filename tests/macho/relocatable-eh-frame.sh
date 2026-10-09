#!/bin/bash
source "$(dirname "$0")"/common.inc

# DWARF-only unwind info exists on x86-64; arm64 compilers always
# emit compact unwind.
[ $ARCH = x86_64 ] || skip

# .cfi_escape defeats compact-unwind encoding, so this frame's unwind
# info exists only as a DWARF FDE; .cfi_personality gives its CIE a
# personality, exercising the one relocation __eh_frame carries.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _through_asm
_through_asm:
 .cfi_startproc
 .cfi_personality 155, ___gxx_personality_v0
 .cfi_escape 0x00
 pushq %rbp
 .cfi_def_cfa_offset 16
 .cfi_offset %rbp, -16
 movq %rsp, %rbp
 .cfi_def_cfa_register %rbp
 callq _thrower
 popq %rbp
 retq
 .cfi_endproc
.subsections_via_symbols
EOF

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
#include <cstdio>
extern "C" void thrower() { throw 40; }
extern "C" void through_asm();
int main() { try { through_asm(); } catch (int e) { printf("caught %d\n", e + 2); } }
EOF

$mold -r -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 15.0 15.0} -o $t/merged.o $t/a.o $t/b.o

# The merged object carries __eh_frame with the personality's GOT
# relocation, the shape compilers emit.
otool -l $t/merged.o | grep 'sectname __eh_frame'
otool -r $t/merged.o > $t/relocs
sed -n '/__eh_frame/,+3p' $t/relocs | grep '1     2      1      4'

# The assembler gave the frame a DWARF-mode compact unwind record;
# ld64 -r copies it as it came (the next link regenerates its
# encoding from the FDE). The function and LSDA fields refer to their
# sections, as clang writes them; the personality is named.
otool -l $t/merged.o | grep -A3 'sectname __compact_unwind' | grep 'size 0x0000000000000060'
otool -rv $t/merged.o | sed -n '/__compact_unwind/,/^Rel/p' > $t/cu_relocs
grep -q '^00000000 .*False  UNSIGND False     1 (__TEXT,__text)$' $t/cu_relocs
grep -q 'True   UNSIGND False     ___gxx_personality_v0$' $t/cu_relocs
python3 - $t/merged.o <<'EOF2'
import struct, sys, macho
m = macho.MachO(sys.argv[1])
data = m.contents(m.section('__compact_unwind'))
encs = [struct.unpack_from('<I', data, e + 12)[0] for e in range(0, len(data), 32)]
assert 0x04000000 in encs, [hex(e) for e in encs]  # UNWIND_X86_64_MODE_DWARF
EOF2

# ld-prime -r carries every input CIE and FDE through (the compactly-
# encoded functions' too) under the section's conventional flags and
# names none of them: the CIE pointer, pc_begin and LSDA fields are
# recomputed self-relative values, and the only relocation is the
# personality's GOT reference.
otool -l $t/merged.o | grep -A8 'sectname __eh_frame' | grep 'flags 0x6800000b'
nm -xp $t/merged.o | awk '{print $NF}' > $t/names
not grep -q '^EH_Frame1$' $t/names
not grep -q '^func.eh$' $t/names
otool -rv $t/merged.o | sed -n '/__eh_frame/,/^Relocation information (__/p' > $t/eh_relocs
not grep -q 'SUB ' $t/eh_relocs
[ "$(grep -c 'GOT' $t/eh_relocs)" -ge 1 ]
not grep -q '_through_asm' $t/eh_relocs

# The exception unwinds through the assembly frame after a final link
# by either linker.
$CXX --ld-path=$mold -o $t/exe $t/merged.o
$RUN $t/exe | grep 'caught 42'
$CXX -o $t/exe2 $t/merged.o
$RUN $t/exe2 | grep 'caught 42'
