#!/usr/bin/env bash
. $(dirname $0)/common.inc

echo 'char data[8];' | $CC -dynamiclib -xc - -o $t/libdata.dylib
cat <<EOF | $CC -c -xassembler - -o $t/pointers.o
.data
.globl _pointers
.p2align 3
_pointers:
.quad _data + 0x7fffffff
.quad _data + 0x80000000
.quad _data + 0xffffffff
.quad _data - 0x80000000
.quad _data - 0x80000001
EOF
cat <<EOF | $CC -c -xc - -o $t/main.o
#include <stdio.h>
#include <stdint.h>
extern char data[], *pointers[];
int main() {
  for (int i = 0; i < 5; i++)
    printf("%lld\n", (long long)((uintptr_t)pointers[i] - (uintptr_t)data));
}
EOF
printf '2147483647\n2147483648\n4294967295\n-2147483648\n-2147483649\n' > $t/expected
$CC --ld-path=$mold $t/main.o $t/pointers.o $t/libdata.dylib -Wl,-fixup_chains -o $t/exe
$t/exe > $t/log
cmp $t/log $t/expected

# A positive out-of-range addend alone must select ADDEND64 too.
# A negative addend in the same table used to hide this bug by forcing it.
for addend in 0x7fffffff 0x80000000 0xffffffff -0x80000000; do
  cat <<EOF | $CC -c -xassembler - -o $t/single.o
.data
.globl _value
.p2align 3
_value:
.quad _data + $addend
EOF
  cat <<EOF | $CC -c -xc - -o $t/check.o
#include <stdint.h>
extern char data[], *value;
int main() { return (uintptr_t)value - (uintptr_t)data != (uintptr_t)(${addend}LL); }
EOF
  $CC --ld-path=$mold $t/check.o $t/single.o $t/libdata.dylib -Wl,-fixup_chains -o $t/single
  $t/single
done
