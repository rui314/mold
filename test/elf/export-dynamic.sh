#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl foo
  .hidden foo
foo:
  nop
  .globl bar
bar:
  nop
  .globl _start
_start:
  nop
EOF

cc -shared -fPIC -o $t/b.so -xc /dev/null
$mold -o $t/exe $t/a.o $t/b.so --export-dynamic

readelf --dyn-syms $t/exe > $t/log
fgrep -q 'NOTYPE  GLOBAL DEFAULT    6 bar' $t/log
fgrep -q 'NOTYPE  GLOBAL DEFAULT    6 _start' $t/log

echo OK
