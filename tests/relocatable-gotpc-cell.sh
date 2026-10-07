#!/bin/bash
source "$(dirname "$0")"/common.inc

# A 4-byte pcrel GOT reference - a CIE's personality pointer, an LSDA's
# type info, or a hand-written `.long _x@GOT - .` - goes through -r
# with the bytes its object has under it: an arm64 one
# (ARM64_RELOC_POINTER_TO_GOT) carries no addend, and an x86-64 one
# (X86_64_RELOC_GOT) holds its addend there, so a later link reads the
# reference as it would the object's. (ld-prime writes 4 under each
# arm64 one, and under an x86-64 CIE's personality cell.)
cat <<EOF | $CXX -o $t/a.o -c -xc++ - -fasynchronous-unwind-tables -femit-dwarf-unwind=always
#include <stdexcept>
int f(int x) {
  try { if (x) throw std::runtime_error("e"); } catch (std::exception &) { return 1; }
  return 0;
}
EOF
if [ "$ARCH" = x86_64 ]; then
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__gotrefs
.globl _r1
_r1: .long _ext1@GOTPCREL + 4
EOF
  type=4
else
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __DATA,__gotrefs
.globl _r1
_r1: .long _ext1@GOT - .
.subsections_via_symbols
EOF
  type=7
fi
$mold -arch $ARCH -r $t/a.o $t/b.o -o $t/r.o

# The 4 bytes under each GOT relocation of a section of an object.
cells() {
  local off=$(otool -l $1 | awk -v s=$3 '$1 == "sectname" { n = $2 }
    $1 == "offset" && n == s { print $2; exit }')
  otool -r $1 | awk -v s="($2,$3)" -v type=$type '/^Relocation information/ { in_s = ($3 == s) }
    in_s && $5 == type { print $1 }' | while read addr; do
    xxd -s $((off + 0x$addr)) -l 4 -p $1
  done | sort
}
[ -n "$(cells $t/r.o __TEXT __eh_frame)" ]
[ "$(cells $t/r.o __TEXT __eh_frame)" = "$(cells $t/a.o __TEXT __eh_frame)" ]
[ "$(cells $t/r.o __TEXT __gcc_except_tab)" = "$(cells $t/a.o __TEXT __gcc_except_tab)" ]
[ "$(cells $t/r.o __DATA __gotrefs)" = "$(cells $t/b.o __DATA __gotrefs)" ]

# The references find their GOT slots in a program linked from the
# output, with either linker, and the exception is caught.
cat <<EOF | $CXX -o $t/main.o -c -xc++ -
#include <cstdio>
int f(int);
extern "C" int ext1, r1;
int ext1 = 5;
int main() {
  int **slot = (int **)((char *)&r1 + r1);
  printf("%d %d\n", f(1), **slot);
}
EOF
$CXX --ld-path=$mold -o $t/exe $t/main.o $t/r.o
$RUN $t/exe | grep -q '^1 5$'
$CXX -o $t/exe2 $t/main.o $t/r.o
$RUN $t/exe2 | grep -q '^1 5$'
