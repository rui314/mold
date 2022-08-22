#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
void foo() {}
EOF

rm -f $t/b.a
ar rcs $t/b.a $t/a.o

cat <<EOF | cc -o $t/c.o -c -xc -
int main() {}
EOF

cc --ld-path=./ld64 -o $t/exe1 $t/b.a $t/c.o
nm $t/exe1 > $t/log1
! grep -q _foo $t/log1 || false

cc --ld-path=./ld64 -o $t/exe2 $t/b.a $t/c.o -Wl,-u,_foo
nm $t/exe2 > $t/log2
grep -q _foo $t/log2

echo OK
