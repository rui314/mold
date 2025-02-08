#!/bin/bash
. $(dirname $0)/common.inc

[[ $MACHINE = ppc64* ]] && skip
[[ $MACHINE = loongarch* ]] && skip

cat <<EOF | $CC -o $t/a.o -c -xc -fno-PIE -
#include <stdio.h>

extern char msg[100];

int main() {
  printf("%s\n", msg);
}
EOF

cat <<EOF | $CC -B. -fPIC -shared -o $t/b.so -xc -
__attribute__((section (".data.rel.ro"))) char msg[100] = "Hello world";
EOF

$CC -B. $t/a.o $t/b.so -o $t/exe1 -no-pie -Wl,-z,relro
readelf -W --sections $t/exe1 | grep -Fq .copyrel.rel.ro

$CC -B. $t/a.o $t/b.so -o $t/exe2 -no-pie -Wl,-z,norelro
readelf -W --sections $t/exe2 | not grep -Fq .copyrel.rel.ro
