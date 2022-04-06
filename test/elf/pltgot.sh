#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

[ $MACHINE = x86_64 ] || { echo skipped; exit; }

cat <<EOF | $CC -fPIC -shared -o $t/a.so -x assembler -
.globl ext1, ext2
ext1:
  nop
ext2:
  nop
EOF

cat <<EOF | $CC -c -o $t/b.o -x assembler -
.globl _start
_start:
  call ext1@PLT
  call ext2@PLT
  mov ext2@GOTPCREL(%rip), %rax
  ret
EOF

"$mold" --pie -o $t/exe $t/b.o $t/a.so

$OBJDUMP -d -j .plt.got $t/exe > $t/log

grep -Eq '1020:.*jmp.* <ext2>' $t/log

echo OK
