#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -c -o $t/a.o -x assembler -
.section .data.foo.1,"aw",@progbits
.ascii "a"
.section .data.foo.1,"aw",@progbits
.ascii "b"
.section .data.foo.2,"aw",@progbits
.ascii "c"
.section .data.bar.1,"aw",@progbits
.ascii "d"
.section .data.bar.2,"aw",@progbits
.ascii "e"
.text
.globl _start
_start:
  nop
EOF

clang -fuse-ld=$mold -o $t/exe $t/a.o -nostdlib -Wl,-unique='*foo*'

readelf -x .data.foo.1 $t/exe | grep -q ab
readelf -x .data.foo.2 $t/exe | grep -q c
readelf -x .data $t/exe | grep -q de

echo OK
