#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -x assembler -
.globl foo
foo: .long 42
EOF

cat <<EOF | $CC -m32 -c -o $t/b.o -x assembler - || skip
.globl foo
foo: .long 43
EOF

cat <<EOF | $CC -c -o $t/c.o -x assembler -
.globl bar
bar: .long 44
EOF

mkdir -p $t/bad $t/good
echo 'INPUT(inner.script)' > $t/bad/outer.script
echo 'INPUT(../b.o)' > $t/bad/inner.script
echo 'INPUT(../a.o)' > $t/good/inner.script

# Skip the i386 script beside outer.script and search the -L directory.
./mold -r -m elf_x86_64 -o $t/d.o $t/c.o -L$t/good $t/bad/outer.script
readelf -h $t/d.o | grep -F 'Advanced Micro Devices X86-64'
nm $t/d.o > $t/symbols
grep -E ' T foo$' $t/symbols
grep -E ' T bar$' $t/symbols
