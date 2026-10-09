#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

# The pass timers, as mold's --perf prints them, then the input and
# output sizes.
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-print_statistics 2> $t/log
$RUN $t/exe
grep -q 'User   System     Real  Name' $t/log
grep -q ' all$' $t/log
grep -q ' copy_chunks$' $t/log
grep -Eq 'objects: 1 alive of [0-9]+' $t/log

# Off by default.
$CC --ld-path=$mold -o $t/exe $t/a.o 2> $t/log2
! grep -q 'Real  Name' $t/log2 || false
