#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ "$CC" = cc ] || skip

# ASAN doesn't work with LD_PRELOAD
nm mold | grep '__[at]san_init' && skip

clang --version >& /dev/null || skip

cat <<'EOF' | $CC -xc -c -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

./mold -run clang -no-pie -o $t/exe $t/a.o -fuse-ld=/usr/bin/ld

readelf -p .comment $t/exe | grep mold
