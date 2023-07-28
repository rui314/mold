#!/bin/bash
. $(dirname $0)/common.inc

[ "$CC" = cc ] || skip

# ASAN doesn't work with LD_PRELOAD
nm mold-wrapper.so | grep -q '__[at]san_init' && skip

clang --version >& /dev/null || skip

cat <<'EOF' | $CC -xc -c -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

LD_PRELOAD=`pwd`/mold-wrapper.so MOLD_PATH=`pwd`/mold \
  clang -no-pie -o $t/exe $t/a.o -fuse-ld=/usr/bin/ld
readelf -p .comment $t/exe > $t/log
grep -q '[ms]old' $t/log
