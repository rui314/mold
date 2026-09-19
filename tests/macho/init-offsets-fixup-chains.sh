#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CXX -o $t/a.o -c -xc++ -
#include <iostream>
int foo() { std::cout << "foo "; return 3; }
int x = foo();
int main() {}
EOF

# -fixup_chains implies -init_offsets
$CXX --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-no_fixup_chains
objdump -h $t/exe1 > $t/log1
! grep -q __init_offsets $t/log1 || false

$CXX --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-fixup_chains
objdump -h $t/exe2 | grep -q __init_offsets
