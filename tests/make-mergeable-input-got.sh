#!/bin/bash
source "$(dirname "$0")"/common.inc

# An object may bring a GOT of its own, a __DATA,__got of pointers its
# code loads, which the link moves into the image's GOT. A mergeable
# dylib records each slot in its place, as a GOT slot ("got" content
# type, an entry of its own, not merged by content), so that a merging
# link, the linker or ld-prime, takes it as the object had it.
if [ $ARCH = arm64 ]; then
  cat > $t/a.s <<EOF
.text
.globl _getsum
.p2align 2
_getsum:
  adrp x8, l_slot@PAGE
  ldr x8, [x8, l_slot@PAGEOFF]
  ldr w0, [x8]
  adrp x9, l_slot2@PAGE
  ldr x9, [x9, l_slot2@PAGEOFF]
  ldr w9, [x9]
  add w0, w0, w9
  ret
EOF
else
  cat > $t/a.s <<EOF
.text
.globl _getsum
_getsum:
  movq l_slot(%rip), %rax
  movl (%rax), %eax
  movq l_slot2(%rip), %rcx
  addl (%rcx), %eax
  retq
EOF
fi
cat >> $t/a.s <<EOF
.data
.globl _lvar
.p2align 2
_lvar: .long 100
.section __DATA,__got
.p2align 3
l_slot: .quad _lvar
l_slot2: .quad _imp_var
.subsections_via_symbols
EOF
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/imp.o -c -xc -
int imp_var = 7;
EOF
$CC -shared -o $t/libimp.dylib $t/imp.o -Wl,-install_name,$t/libimp.dylib

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int getsum(void);
int main() { printf("%d\n", getsum()); }
EOF

mkdir -p $t/m $t/l
$CC --ld-path=$mold -shared -o $t/m/libfoo.dylib $t/a.o -L$t -limp -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib
$CC -shared -o $t/l/libfoo.dylib $t/a.o -L$t -limp -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib

# The record's entries of content type "got" (22, bits 8-14 of their
# flags).
python3 - $t/m/libfoo.dylib > $t/got <<'EOF'
import struct, sys
data = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', data, 16)[0]):
    cmd, size, dataoff = struct.unpack_from('<III', data, off)
    if cmd == 0x36:
        b = data[dataoff:]
    off += size
nents, count = struct.unpack_from('<II', b, 0x60)
flags = [struct.unpack_from('<I', b, nents + 40 * i + 16)[0] for i in range(count)]
print(sum(1 for f in flags if f >> 8 & 0x7f == 22))
EOF
grep -q '^2$' $t/got

$CC --ld-path=$mold -o $t/exe1 $t/main.o -L$t/m -Wl,-merge-lfoo
$t/exe1 | grep -q '^107$'
$CC -o $t/exe2 $t/main.o -L$t/m -Wl,-merge-lfoo
$t/exe2 | grep -q '^107$'
$CC --ld-path=$mold -o $t/exe3 $t/main.o -L$t/l -Wl,-merge-lfoo
$t/exe3 | grep -q '^107$'
