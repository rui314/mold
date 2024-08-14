#!/bin/bash
. $(dirname $0)/common.inc

command -v dwarfdump >& /dev/null || skip

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib
dwarfdump $t/exe > $t/log
grep -Fq '.debug_info SHF_COMPRESSED' $t/log
grep -Fq '.debug_str SHF_COMPRESSED' $t/log
