#!/bin/bash
source "$(dirname "$0")"/common.inc

[ "$ARCH" = x86_64 ] || skip

# A one-byte branch (jmp rel8) to a symbol is a 1-byte pcrel BRANCH
# relocation. The assembler writes a jmp to another symbol as a 4-byte
# one, so the test rewrites it: relocation 0 of __text gets length 0,
# the jmp opcode 0xeb, and nops after its displacement byte.
cat > $t/patch.py <<'EOF2'
import sys, macho
m = macho.MachO(sys.argv[1])
text = m.section('__text')
roff, addr, info = m.relocs(text)[0]
m.set_u32(roff + 4, info & ~(3 << 25))
m.data[text.offset + addr - 1] = 0xeb
m.set_u32(text.offset + addr, 0x90909000)
m.save(sys.argv[2])
EOF2

# The jmp becomes one, with three nops after it.
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
python3 $t/patch.py $t/e.o $t/e1.o
$CC --ld-path=$mold -o $t/exe $t/e1.o
code=0
$RUN $t/exe || code=$?
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
python3 $t/patch.py $t/f.o $t/f1.o
not $CC --ld-path=$mold -o $t/exe $t/f1.o 2> $t/log
grep -Eq "$t/f1.o: _main\+0x6: 8-bit branch out of range \(displacement=306, max is \+/-127\), from 0x[0-9A-F]+ to 0x[0-9A-F]+ \('_g'\)" $t/log

cat <<EOF | $CC -o $t/ext.o -c -xc -
int ext = 42;
EOF
$CC --ld-path=$mold -shared -o $t/libext.dylib $t/ext.o
cat <<EOF | $CC -o $t/g.o -c -xassembler -
.text
.globl _main
_main:
  jmp _ext
  ret
.subsections_via_symbols
EOF
python3 $t/patch.py $t/g.o $t/g1.o
not $CC --ld-path=$mold -o $t/exe $t/g1.o $t/libext.dylib 2> $t/log
grep -qF "$t/g1.o: _main+0x1: target '_ext' does not have address" $t/log
