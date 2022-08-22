#!/bin/bash
export LC_ALL=C
set -e
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/macho/$(uname -m)/$testname
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -xassembler -
.globl _foo
.weak_def_can_be_hidden _foo
.p2align 2
_foo:
  ret
EOF

cat <<EOF | cc -o $t/b.o -c -xassembler -
.globl _foo
.weak_definition _foo
.p2align 2
_foo:
  ret
EOF

cat <<EOF | cc -o $t/c.o -c -xc -
int main() {}
EOF

cc --ld-path=./ld64 -o $t/exe $t/a.o $t/b.o $t/c.o
objdump --macho --exports-trie $t/exe | grep -q _foo

echo OK
