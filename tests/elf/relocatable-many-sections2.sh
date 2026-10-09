#!/usr/bin/env bash
. $(dirname $0)/common.inc

# With SHN_LORESERVE (65280) or more sections, e_shnum must be 0 and the
# real number is stored in the first section header's sh_size.
seq 1 65400 | sed 's/.*/int x&=1;/' | $CC -c -xc -fdata-sections -o $t/a.o -

cat <<'EOF' | $CC -c -xc -o $t/b.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

./mold -r -o $t/c.o $t/a.o $t/b.o
readelf -h $t/c.o | grep -E 'Number of section headers: +0 \(654[0-9]{2}\)'

$CC -B. -o $t/exe $t/c.o
$QEMU $t/exe | grep Hello
