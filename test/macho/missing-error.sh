#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
int foo();

int main() {
  foo();
}
EOF

! cc --ld-path=./ld64 -o $t/exe $t/a.o 2> $t/log || false
grep -q 'undefined symbol: .*\.o: _foo' $t/log

echo OK
