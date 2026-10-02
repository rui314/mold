#!/bin/bash
source "$(dirname "$0")"/common.inc

# A C++ inline function's static local is one variable, however many
# translation units have a copy of the function. When a ThinLTO module
# and the merged module both have one, the copy that loses has to reach
# the one that won, the variable with it, in either order. ld-prime
# leaves each module its own and prints 1 for both orders, which breaks
# the program; this test checks mold's behavior only.
cat <<EOF > $t/h.h
__attribute__((noinline)) inline int counter() { static int k = 0; return ++k; }
EOF
cat <<EOF | $CXX -O2 -flto=thin -I$t -o $t/a-thin.o -c -xc++ -
#include "h.h"
__attribute__((noinline)) int fa() { return counter(); }
EOF
cat <<EOF | $CXX -O2 -flto -I$t -o $t/b-full.o -c -xc++ -
#include "h.h"
__attribute__((noinline)) int fb() { return counter(); }
EOF
cat <<EOF | $CXX -O2 -flto -I$t -o $t/a-full.o -c -xc++ -
#include "h.h"
__attribute__((noinline)) int fa() { return counter(); }
EOF
cat <<EOF | $CXX -O2 -flto=thin -I$t -o $t/b-thin.o -c -xc++ -
#include "h.h"
__attribute__((noinline)) int fb() { return counter(); }
EOF
cat <<EOF | $CXX -O2 -o $t/main.o -c -xc++ -
#include <stdio.h>
int fa(), fb();
int main() { fa(); fa(); printf("%d\n", fb()); }
EOF

$CXX --ld-path=$mold -o $t/exe1 $t/main.o $t/a-thin.o $t/b-full.o
$t/exe1 | grep -q '^3$'
$CXX --ld-path=$mold -o $t/exe2 $t/main.o $t/a-full.o $t/b-thin.o
$t/exe2 | grep -q '^3$'
