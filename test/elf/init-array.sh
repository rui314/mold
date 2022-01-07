#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

cat <<EOF | cc -c -o "$t"/a.o -x assembler -
.globl init1, fini1

.section .INIT_ARRAY,"aw",@progbits
.p2align 3
.quad init1

.section .FINI_ARRAY,"aw",@progbits
.p2align 3
.quad fini1
EOF

# GNU as complains if we set PROGBITS to .{init,fini}_array, so we
# rewrite the section names after GNU as generates an object file.
sed -i 's/INIT_ARRAY/init_array/g; s/FINI_ARRAY/fini_array/g;' "$t"/a.o

cat <<EOF | cc -c -o "$t"/b.o -x assembler -
.globl init2, fini2

.section .init_array,"aw",@init_array
.p2align 3
.quad init2

.section .fini_array,"aw",@fini_array
.p2align 3
.quad fini2
EOF

cat <<EOF | cc -c -o "$t"/c.o -xc -
#include <stdio.h>

void init1() { printf("init1 "); }
void init2() { printf("init2 "); }
void fini1() { printf("fini1\n"); }
void fini2() { printf("fini2 "); }

int main() {
  return 0;
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o "$t"/c.o
"$t"/exe | grep -q 'init1 init2 fini2 fini1'

echo OK
