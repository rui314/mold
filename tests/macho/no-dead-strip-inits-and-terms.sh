#!/bin/bash
source "$(dirname "$0")"/common.inc

# Given with -dead_strip, -no_dead_strip_inits_and_terms once kept
# initializers and terminators nothing referenced. -dead_strip keeps
# them anyway now, so ld64 takes the option for -dead_strip alone and
# warns that it is obsolete.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__attribute__((constructor)) static void ctor(void) { printf("ctor\n"); }
__attribute__((destructor)) static void dtor(void) { printf("dtor\n"); }
void dead(void) {}
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-no_dead_strip_inits_and_terms 2> $t/log
grep -q "option '-no_dead_strip_inits_and_terms' is obsolete, use '-dead_strip' instead" $t/log
nm $t/exe > $t/syms
not grep -q ' _dead$' $t/syms
grep -q ' _ctor$' $t/syms
grep -q ' _dtor$' $t/syms
[ "$($RUN $t/exe | tr '\n' ' ')" = 'ctor dtor ' ]

# That makes it an error with -r.
not $mold -r -arch $ARCH -o $t/r.o $t/a.o -no_dead_strip_inits_and_terms 2> $t/log2
grep -q -- '-r and -dead_strip cannot be used together' $t/log2
