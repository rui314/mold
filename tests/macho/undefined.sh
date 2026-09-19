#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void hello();
int main() { hello(); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-flat_namespace -Wl,-undefined,warning
objdump --macho --bind --lazy-bind $t/exe | grep -Eq '\sflat-namespace\s+_hello'
