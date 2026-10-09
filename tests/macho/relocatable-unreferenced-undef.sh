#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output keeps every undefined symbol its inputs list, and those
# -u names, as a link of the inputs would see them: a stray .globl, a
# weak, lazy or no-dead-strip reference nothing relocates too. A final
# link ignores those. (ld-prime keeps only those a relocation, an
# unwind personality or -u names.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _g2
.weak_reference _g4
.reference _g6
.lazy_reference _g7
.text
.globl _f
_f:
  ret
.data
_d:
  .quad _g3
EOF
$mold -r -arch $ARCH -o $t/r.o $t/a.o -u _g8
nm -m $t/r.o > $t/log
for s in _g2 _g3 _g6 _g7 _g8; do
  grep -q "(undefined) external $s\$" $t/log
done
grep -q '(undefined) weak external _g4$' $t/log

cat <<EOF | $CC -o $t/main.o -c -xc -
long g3;
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o
$RUN $t/exe
$CC -o $t/exe2 $t/main.o $t/r.o
$RUN $t/exe2

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
int f();
int g() { try { return f(); } catch (...) { return 0; } }
EOF
$mold -r -arch $ARCH -o $t/r2.o $t/b.o
nm -m $t/r2.o | grep -q '(undefined) external ___gxx_personality_v0$'
