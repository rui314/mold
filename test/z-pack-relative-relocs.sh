#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
#include <stdio.h>
__attribute__((section(".foo"))) void *ptr = &ptr;
int main() {
  printf("Hello world\n");
}
EOF

$CC -o $t/exe1 $t/a.o -pie -Wl,-z,pack-relative-relocs 2> /dev/null || skip
readelf -WS $t/exe1 | grep -F .relr.dyn || skip
$QEMU $t/exe1 2> /dev/null | grep Hello || skip

$CC -B. -o $t/exe2 $t/a.o -pie -Wl,-z,pack-relative-relocs
$QEMU $t/exe2 | grep Hello

readelf --dynamic $t/exe2 > $t/log2
grep -Ew 'RELR|<unknown>: 24' $t/log2
grep -Ew 'RELRSZ|<unknown>: 23' $t/log2
grep -Ew 'RELRENT|<unknown>: 25' $t/log2

# An explicit section address may violate the section's usual alignment.
# Such a relative relocation is not representable in RELR.
$CC -B. -o $t/exe3 $t/a.o -pie -Wl,-z,pack-relative-relocs \
  -Wl,--section-start=.foo=0x100001
# `exit` in awk would close the pipe early and kill readelf with SIGPIPE,
# which pipefail turns into a test failure.
ptr_addr=$(readelf -sW $t/exe3 | awk '$NF == "ptr" && !found { print $2; found = 1 }')
readelf -rW $t/exe3 | grep -E \
  "^[[:space:]]*$ptr_addr[[:space:]].*R_.*_RELATIVE"
