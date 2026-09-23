#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void live() { printf("live\n"); }
void dead() { printf("dead\n"); }
int dead_data[1000] = {3};
int main() { live(); }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-dead_strip
$t/exe | grep '^live$'

nm $t/exe > $t/syms
grep -q _live $t/syms
not grep -q ' _dead' $t/syms

# An import only stripped code used is gone from the symbol table too.
cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <stdlib.h>
#include <unistd.h>
void dead() { _exit(getpid()); }
int main() { return 0; }
EOF2
$CC --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-dead_strip
nm -m $t/exe2 > $t/syms2
not grep -q '_getpid' $t/syms2
not grep -q '__exit' $t/syms2
# ...but one live code still binds is listed.
$CC --ld-path=$mold -o $t/exe3 $t/b.o
nm -m $t/exe3 | grep 'undefined.*_getpid'
