#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xc -
void foo() {}
__attribute__((visibility("hidden"))) void bar() {}
EOF

cc --ld-path=./ld64 -shared -o $t/b.dylib $t/a.o
objdump --macho --exports-trie $t/b.dylib > $t/log
grep -q _foo $t/log
! grep -q _bar $t/log || false

echo OK
