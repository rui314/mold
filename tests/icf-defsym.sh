#!/usr/bin/env bash
. $(dirname $0)/common.inc

# f1 and f2 are identical except that they refer to different --defsym'd
# symbols, so ICF must not merge them.
cat <<EOF | $CC -fPIC -c -o $t/a.o -O2 -ffunction-sections -xc -
extern char foo;
extern char bar;
void *f1(void) { return &foo; }
void *f2(void) { return &bar; }
EOF

cat <<EOF | $CC -c -o $t/b.o -xc -
#include <stdio.h>

void *f1(void);
void *f2(void);

int main() {
  printf("%p %p\n", f1(), f2());
}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--icf=all -Wl,-defsym=foo=0x1000 \
  -Wl,-defsym=bar=0x2000
$QEMU $t/exe | grep '^0x1000 0x2000$'
