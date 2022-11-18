#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
int main() {}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o -Wl,-dependency_info,$t/dep

grep -q '[ms]old' $t/dep
grep -q "\x10$t/a.o" $t/dep
grep -q "@$t/exe" $t/dep

echo OK
