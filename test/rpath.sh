#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl _start
_start:
  nop
EOF

../mold -o $t/exe $t/a.o -rpath /foo -rpath /bar

readelf --dynamic $t/exe | grep -q "
0x000000000000001d (RUNPATH) Library runpath: [/foo]
0x000000000000001d (RUNPATH) Library runpath: [/bar]
"

echo OK
