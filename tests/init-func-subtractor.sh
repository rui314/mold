#!/bin/bash
source "$(dirname "$0")"/common.inc

# An initializer pointer the difference of two symbols makes, a
# SUBTRACTOR and an UNSIGNED relocation, names the function it adds:
# ld-prime runs (and lists for -no_inits) that one alone.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
void f(void) { printf("f\n"); }
void g(void) { printf("g\n"); }
__asm__(".section __DATA,__mod_init_func,mod_init_funcs\n"
        ".p2align 3\n"
        ".quad _f - _g\n"
        ".text");
int main() { printf("main\n"); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$RUN $t/exe > $t/out
printf 'f\nmain\n' | cmp - $t/out

not $CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_inits 2> $t/log
grep -q '^_f in ' $t/log
not grep -q '^_g in ' $t/log
