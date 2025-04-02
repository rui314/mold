#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = $(uname -m) ] || skip

echo '' | clang -S -emit-llvm -flto -o $t/a.ll -xc - || skip

cp $t/a.ll $t/b.ll

cat <<'EOF' >> $t/b.ll
$foo = comdat any
@foo = global i32 42, comdat($foo)
EOF

cp $t/b.ll $t/c.ll

clang -S -emit-llvm -flto -o $t/b.bc $t/b.ll
clang -S -emit-llvm -flto -o $t/c.bc $t/c.ll

cat <<'EOF' | clang -o $t/d.o -c -flto -xc -
#include <stdio.h>
extern int foo;
int main() { printf("%d\n", foo); }
EOF

clang -B. -o $t/exe -flto $t/b.bc $t/c.bc $t/d.o
$QEMU $t/exe | grep 42
