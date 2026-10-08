#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fPIE
#include <stdio.h>

void hello() {
  puts("Hello world");
}

__attribute__((section(".text")))
void (*p)() = hello;

int main() {
  p();
}
EOF

$CC -o $t/exe1 $t/a.o -pie -Wl,-z,notext
$QEMU $t/exe1 || skip

$CC -B. -o $t/exe2 $t/a.o -pie
$QEMU $t/exe2 | grep 'Hello world'

$CC -o $t/exe3 $t/a.o -pie -Wl,-z,notext -Wl,-z,pack-relative-relocs 2> /dev/null || skip
readelf -WS $t/exe3 | grep -F .relr.dyn || skip
$QEMU $t/exe3 2> /dev/null | grep 'Hello world' || skip

$CC -B. -o $t/exe4 $t/a.o -pie -Wl,-z,pack-relative-relocs
readelf -WS $t/exe4 | grep -F .relr.dyn

# Keep relative relocations in executable sections out of RELR because their
# offsets may change during relaxation or thunk removal.
# `exit` in awk would close the pipe early and kill readelf with SIGPIPE,
# which pipefail turns into a test failure.
p_addr=$(readelf -sW $t/exe4 | awk '$NF == "p" && !found { print $2; found = 1 }')
readelf -rW $t/exe4 | grep -E "^[[:space:]]*$p_addr[[:space:]].*R_.*_RELATIVE"

$QEMU $t/exe4 | grep 'Hello world'
