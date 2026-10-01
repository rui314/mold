#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime lays __eh_frame out as its inputs have it: object by object,
# the CIEs and FDEs of each in their order there, so the CIE of an
# object follows the FDEs of the one before it. (A DW_CFA_nop, which
# compact unwind can't express, gives each function an FDE.)
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
$t/exe

python3 - $t/exe > $t/records <<'EOF'
import struct, subprocess, sys
out = subprocess.run(['otool', '-l', sys.argv[1]], capture_output=True, text=True).stdout.splitlines()
for i, l in enumerate(out):
    if l.strip() == 'sectname __eh_frame':
        size = int(out[i + 3].split()[1], 16); off = int(out[i + 4].split()[1])
d = open(sys.argv[1], 'rb').read()[off:off + size]
pos = 0
while pos < len(d):
    length, id = struct.unpack_from('<II', d, pos)
    print('FDE' if id else 'CIE', end=' ')
    pos += 4 + length
EOF
[ "$(cat $t/records)" = 'CIE FDE CIE FDE ' ]
