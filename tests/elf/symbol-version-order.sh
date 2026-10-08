#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Versions of a symbol have the same name and thus the same .gnu.hash
# bucket. They must appear in .dynsym in their input order, so that the
# output is the same on every host. Create enough of them that a sort
# would reorder equal keys.
for i in $(seq 1 1000); do
  echo ".globl foo${i}_1, foo${i}_2, foo${i}_3"
  echo "foo${i}_1:"
  echo "foo${i}_2:"
  echo "foo${i}_3:"
  echo ".symver foo${i}_1, foo$i@V1"
  echo ".symver foo${i}_2, foo$i@V2"
  echo ".symver foo${i}_3, foo$i@@V3"
done | $CC -c -xassembler -o $t/a.o -

echo 'V1 {}; V2 {}; V3 {};' > $t/b.ver
$CC -B. -shared -o $t/c.so $t/a.o -Wl,--version-script=$t/b.ver

readelf -W --dyn-syms $t/c.so | grep -oE 'foo[0-9]+@@?V[0-9]' |
  paste -d' ' - - - > $t/log
[ $(wc -l < $t/log) = 1000 ]
not grep -v '@V1 .*@V2 .*@@V3$' $t/log
