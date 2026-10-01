#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output passes a tentative definition on, with an N_GSYM for it
# among its debug notes. A final link that merges several such
# definitions notes the symbol once, in the first object with notes
# that declares it, not in each.
for i in 1 2 3; do
  cat <<EOF | $CC -g -fcommon -o $t/a$i.o -c -xc -
int shared_common;
int f$i(void) { return shared_common + $i; }
EOF
  $mold -r -arch $ARCH -o $t/r$i.o $t/a$i.o
done
echo 'int f1(void), f2(void), f3(void); int main() { return f1() + f2() + f3(); }' | \
  $CC -o $t/main.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/main.o $t/r1.o $t/r2.o $t/r3.o
nm -ap $t/exe > $t/log
[ "$(grep -c 'GSYM _shared_common$' $t/log)" = 1 ]
grep -E ' OSO | GSYM _shared_common$' $t/log | grep -B1 GSYM | head -1 | grep -q 'a1.o$'
