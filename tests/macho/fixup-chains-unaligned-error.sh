#!/bin/bash
source "$(dirname "$0")"/common.inc

# foo sits at offset 1 in __data, so the pointer inside it cannot be a
# link in a fixup chain (stride is 4 bytes).
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.globl _foo
.data
.byte 0
_foo:
.quad _bar
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int bar = 3;
int main() {}
EOF

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-fixup_chains >& $t/log
grep -Fq '/a.o(__DATA,__data): unaligned base relocation' $t/log

# Classic rebase opcodes have byte granularity; the same input links.
$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-no_fixup_chains
