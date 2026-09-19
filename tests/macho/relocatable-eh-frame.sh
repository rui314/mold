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

$mold -r -arch $ARCH -platform_version macos 15.0 15.0 -o $t/merged.o $t/a.o $t/b.o

# The merged object carries __eh_frame with the personality's GOT
# relocation, the shape compilers emit.
otool -l $t/merged.o | grep -q 'sectname __eh_frame'
otool -r $t/merged.o > $t/relocs
sed -n '/__eh_frame/,+3p' $t/relocs | grep -q '1     2      1      4'

# The assembler gave the frame a DWARF-mode compact unwind record;
# ld64 -r copies it as it came (the next link regenerates its
# encoding from the FDE), naming the function by an extern
# relocation, as it names every function and LSDA that has a symbol.
otool -l $t/merged.o | grep -A3 'sectname __compact_unwind' | grep -q 'size 0x0000000000000060'
sed -n '/__compact_unwind/,/^Relocation information (__TEXT/p' $t/relocs > $t/cu_relocs
grep -q '^00000000 .* _through_asm$\|^00000000 .*1 *_through_asm' $t/cu_relocs || otool -rv $t/merged.o | sed -n '/__compact_unwind/,/^Rel/p' | grep -q '^00000000 .*True   UNSIGND False     _through_asm'
python3 - $t/merged.o <<'EOF2'
import struct, subprocess, sys
f = sys.argv[1]
out = subprocess.run(['otool', '-l', f], capture_output=True, text=True).stdout.splitlines()
for i, l in enumerate(out):
    if l.strip() == 'sectname __compact_unwind':
        size = int(out[i + 3].split()[1], 16); off = int(out[i + 4].split()[1])
data = open(f, 'rb').read()[off:off + size]
encs = [struct.unpack_from('<I', data, e + 12)[0] for e in range(0, size, 32)]
assert 0x04000000 in encs, [hex(e) for e in encs]  # UNWIND_X86_64_MODE_DWARF
EOF2

# ld64 -r carries every input CIE and FDE (the compactly-encoded
# functions' too), names each CIE EH_Frame1 and each FDE func.eh, and
# writes the FDE's CIE pointer, pc_begin and LSDA fields as SUBTRACTOR
# pairs against those symbols: a.o and b.o bring three FDEs (and a
# CIE each, plus one for b.o's frame without a personality).
nm -xp $t/merged.o | awk '{print $NF}' > $t/names
[ "$(grep -c '^EH_Frame1$' $t/names)" -ge 2 ]
[ "$(grep -c '^func.eh$' $t/names)" = 3 ]
otool -rv $t/merged.o | sed -n '/__eh_frame/,/^Relocation information (__/p' > $t/eh_relocs
[ "$(grep -c 'SUB     False     EH_Frame1' $t/eh_relocs)" = 3 ]
[ "$(grep -c 'SUB     False     func.eh' $t/eh_relocs)" -ge 3 ]
grep -q 'UNSIGND False     _through_asm' $t/eh_relocs

# The exception unwinds through the assembly frame after a final link
# by either linker.
$CXX --ld-path=$mold -o $t/exe $t/merged.o
$t/exe | grep -q 'caught 42'
$CXX -o $t/exe2 $t/merged.o
$t/exe2 | grep -q 'caught 42'
