#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF > $t/a.h
#define A 23
#define B 99
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -I$t -g3
#include "a.h"
extern int z();
int main () { return z() - 122; }
EOF

cat <<EOF | $CC -o $t/c.o -c -xc - -I$t -g3
#include "a.h"
int z()  { return A + B; }
EOF

$CC -B. -o $t/exe $t/b.o $t/c.o
$OBJDUMP --dwarf=macro $t/exe | not grep 'DW_MACRO_import -.* 0x0$'
