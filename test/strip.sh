#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | cc -x assembler -c -o $t/a.o -Wa,--keep-locals -
.globl _start, foo
_start:
foo:
bar:
.L.baz:
EOF

../mold -o $t/exe $t/a.o
readelf --symbols $t/exe > $t/log
fgrep -q _start $t/log
fgrep -q foo $t/log
fgrep -q bar $t/log
fgrep -q .L.baz $t/log

../mold -o $t/exe $t/a.o -strip-all
readelf --symbols $t/exe > $t/log
! fgrep -q _start $t/log || false
! fgrep -q foo $t/log || false
! fgrep -q bar $t/log || false
! fgrep -q .L.baz $t/log || false

echo OK
