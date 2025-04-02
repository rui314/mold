#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = $(uname -m) ] || skip
echo 'int main() {}' | clang -B. -flto -o /dev/null -xc - || skip

echo '' | clang -S -emit-llvm -flto -o $t/a.ll -xc - || skip

cat <<'EOF' >> $t/a.ll
$foo = comdat any
@foo = global i32 42, comdat($foo)
EOF

cp $t/a.ll $t/b.ll

clang -S -emit-llvm -flto -o $t/a.bc $t/a.ll
clang -S -emit-llvm -flto -o $t/b.bc $t/b.ll

cat <<'EOF' | clang -o $t/c.o -c -flto -xc -
#include <stdio.h>
extern int foo;
int main() { printf("%d\n", foo); }
EOF

clang -B. -o $t/exe -flto $t/a.bc $t/b.bc $t/c.o
$QEMU $t/exe | grep 42
