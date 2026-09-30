#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void leaf() {}
void middle() { leaf(); }
void unused() {}
int main() { middle(); }
EOF

# The chain from _leaf back to the entry point, on stderr.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,_leaf 2> $t/log
grep -q '^_leaf from .*/a.o' $t/log
grep -q '^  _middle from .*/a.o' $t/log
grep -q '^    _main from .*/a.o' $t/log

# A dead symbol prints nothing; a wildcard matches several.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,_unused 2> $t/log2
! grep -q _unused $t/log2 || false

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip -Wl,-why_live,'_m*' 2> $t/log3
grep -q '^_middle' $t/log3
grep -q '^_main' $t/log3
