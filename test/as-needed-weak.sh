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
        .weak fn1
        call fn1@PLT
EOF

cat <<EOF | cc -o $t/b.so -shared -fPIC -Wl,-soname,libfoo.so -xc -
int fn1() { return 42; }
EOF

cat <<EOF | cc -o $t/c.so -shared -fPIC -Wl,-soname,libbar.so -xc -
int fn2() { return 42; }
EOF

../mold -o $t/exe $t/a.o $t/b.so $t/c.so

readelf --dynamic $t/exe > $t/readelf
fgrep -q 'Shared library: [libfoo.so]' $t/readelf
fgrep -q 'Shared library: [libbar.so]' $t/readelf

../mold -o $t/exe $t/a.o --as-needed $t/b.so $t/c.so

readelf --dynamic $t/exe > $t/readelf
! fgrep -q 'Shared library: [libfoo.so]' $t/readelf || false
! fgrep -q 'Shared library: [libbar.so]' $t/readelf || false

echo OK
