#!/bin/bash
export LC_ALL=C
set -e
CC="${CC:-cc}"
CXX="${CXX:-c++}"
GCC="${GCC:-gcc}"
GXX="${GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t=out/test/elf/$testname
mkdir -p $t

which dwarfdump >& /dev/null || { echo skipped; exit; }

cat <<EOF | $CC -c -g -o $t/a.o -xc -
#include <stdio.h>

int main() {
  printf("Hello world\n");
  return 0;
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib
dwarfdump $t/exe > $t/log
fgrep -q '.debug_info SHF_COMPRESSED' $t/log
fgrep -q '.debug_str SHF_COMPRESSED' $t/log

$CC -B. -o $t/exe $t/a.o -Wl,--compress-debug-sections=zlib-gnu
dwarfdump $t/exe > $t/log
fgrep -q .zdebug_info $t/log
fgrep -q .zdebug_str $t/log

echo OK
