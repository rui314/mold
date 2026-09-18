#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = sh4 ] && skip
[ $MACHINE = sh4aeb ] && skip

# ARM uses .ARM.exidx instead of .eh_frame.
[[ $MACHINE = arm* ]] && skip
nm mold | grep '__tsan_init' && skip

# a.o has FDEs but no personality routine; b.o's CIE refers to one.
# In the relocatable output, b.o's CIE precedes a.o's FDEs, so its
# personality relocation must be emitted before a.o's FDE relocations.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(int x) { return x + 1; }
int bar(int x) { return x * 2; }
EOF

cat <<EOF | $CXX -o $t/b.o -c -xc++ -
void throws();
int baz() {
  try {
    throws();
  } catch (int x) {
    return x;
  }
  return 0;
}
EOF

./mold -r -o $t/c.o $t/a.o $t/b.o

readelf -rW $t/c.o | sed -nE '/\.rela?\.eh_frame/,/^$/p' | grep '^0' |
  awk '{ print $1 }' > $t/log
sort -c $t/log

cat <<EOF | $CXX -o $t/d.o -c -xc++ -
#include <stdio.h>

void throws() { throw 3; }
int baz();

int main() {
  printf("%d\n", baz());
}
EOF

$CXX -B. -o $t/exe $t/d.o $t/c.o
$QEMU $t/exe | grep '^3$'
