#!/bin/bash
source "$(dirname "$0")"/common.inc

# __eh_frame holds the CIEs the kept FDEs use and then the FDEs, as
# mold's EhFrameSection lays them out: every FDE's CIE pointer, a
# backward offset, leads to a CIE before it. (ld-prime lays the records
# out object by object in their input order.) A DW_CFA_nop, which
# compact unwind can't express, gives each function an FDE.
for i in 1 2; do
  cat <<EOF | $CC -o $t/$i.o -c -xassembler -
.text
.globl _f$i
.p2align 2
_f$i:
  .cfi_startproc
  .cfi_escape 0x0
  ret
  .cfi_endproc
.subsections_via_symbols
EOF
done
echo 'void f1(void), f2(void); int main() { f1(); f2(); }' | $CC -o $t/a.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/a.o $t/1.o $t/2.o
$RUN $t/exe

objdump --macho --unwind-info $t/exe > $t/unwind
python3 - $t/exe $t/unwind $ARCH > $t/records <<'EOF'
import re, struct, sys, macho
image = macho.MachO(sys.argv[1])
d = image.contents(image.section('__eh_frame'))
pos = 0
cies, fdes = set(), set()
while pos < len(d):
    length, id = struct.unpack_from('<II', d, pos)
    if id:
        assert pos + 4 - id in cies, (pos, id, cies)
        fdes.add(pos)
        print('FDE', end=' ')
    else:
        cies.add(pos)
        print('CIE', end=' ')
    pos += 4 + length
# __unwind_info points each function at an FDE of its own (DWARF mode
# with the FDE's offset).
mode = 3 if sys.argv[3] == 'arm64' else 4
encs = [int(m, 16) for m in re.findall(r'encoding\[\d+\]: (0x[0-9a-f]+)', open(sys.argv[2]).read())]
offs = {e & 0xffffff for e in encs if (e >> 24) & 0xf == mode}
assert offs == fdes, (offs, fdes)
EOF
[ "$(cat $t/records)" = 'CIE CIE FDE FDE ' ]
