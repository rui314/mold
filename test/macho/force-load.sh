#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
int foo = 3;
EOF

rm -f $t/b.a
ar rc $t/b.a $t/a.o

cat <<EOF | cc -o $t/c.o -c -xc -
int bar = 5;
EOF

rm -f $t/d.a
ar rc $t/d.a $t/c.o

cat <<EOF | cc -o $t/e.o -c -xc -
int main() {}
EOF

cc --ld-path=./ld64 -o $t/exe $t/e.o -Wl,-force_load,$t/b.a $t/d.a

nm $t/exe > $t/log
grep -q 'D _foo$' $t/log
! grep -q 'D _bar$' $t/log || false

echo OK
