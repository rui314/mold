#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output passes a tentative definition on, with an N_GSYM for it
# among its debug notes. A final link that merges several such
# definitions notes the symbol in each object that declares it, as a
# link of the objects would, and dsymutil takes the notes as they are.
# (ld-prime notes it in the first object with notes only.)
for i in 1 2 3; do
  cat <<EOF | $CC -g -fcommon -o $t/a$i.o -c -xc -
int shared_common;
int f$i(void) { return shared_common + $i; }
EOF
  $mold -r -arch $ARCH -o $t/r$i.o $t/a$i.o
done
echo 'int f1(void), f2(void), f3(void); int main() { return f1() + f2() + f3() != 6; }' | \
  $CC -o $t/main.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/main.o $t/r1.o $t/r2.o $t/r3.o
$RUN $t/exe
nm -ap $t/exe > $t/log
[ "$(grep -c 'GSYM _shared_common$' $t/log)" = 3 ]
for i in 1 2 3; do
  grep -E ' OSO | GSYM _shared_common$' $t/log | grep -A1 "a$i.o\$" | grep -q GSYM
done
dsymutil -o $t/exe.dSYM $t/exe > $t/dsym.log 2>&1
not grep -qi warning $t/dsym.log
