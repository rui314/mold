#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Tentative definitions merge into the largest size and the greatest
# alignment, whatever the input order (see common-merge.sh; ld-prime
# keeps the largest one's alignment).
echo 'char buffer[4096];' | $CC -fcommon -c -xc - -o $t/large.o
echo 'char buffer[1] __attribute__((aligned(4096)));' | $CC -fcommon -c -xc - -o $t/aligned.o
for inputs in "$t/large.o $t/aligned.o" "$t/aligned.o $t/large.o"; do
  $mold -r -arch $ARCH $inputs -o $t/common.o
  nm -m $t/common.o > $t/syms
  grep -E '^0*1000 \(common\) .*external _buffer$' $t/syms
  if is_mold; then
    grep -E '\(alignment 2\^12\) external _buffer$' $t/syms
  fi
done

echo 'int choice() { return 1; }' | $CC -c -xc - -o $t/archive.o
echo 'int choice() { return 2; }' | $CC -dynamiclib -xc - -o $t/libchoice.dylib
rm -f $t/libchoice.a
ar rcs $t/libchoice.a $t/archive.o
cat <<EOF | $CC -c -xc - -o $t/main.o
#include <stdio.h>
int choice();
int main() { printf("%d\n", choice()); }
EOF

$CC --ld-path=$mold $t/main.o $t/libchoice.a $t/libchoice.dylib -o $t/archive-first
$RUN $t/archive-first | grep '^1$'
$CC --ld-path=$mold $t/main.o $t/libchoice.dylib $t/libchoice.a -o $t/dylib-first
$RUN $t/dylib-first | grep '^2$'

# A live weak definition must also outrank a dylib's definition.
echo '__attribute__((weak)) int choice() { return 3; }' | $CC -c -xc - -o $t/weak.o
$CC --ld-path=$mold $t/main.o $t/weak.o $t/libchoice.dylib -o $t/weak
$RUN $t/weak | grep '^3$'
