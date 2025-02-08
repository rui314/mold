#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = $(uname -m) ] || skip

echo 'int main() {}' | clang -B. -flto -o /dev/null -xc - >& /dev/null || skip

echo 'int main() {}' | clang -c -o $t/a.o -xc -
echo 'void foo() {}' | clang -c -o $t/b.o -xc - -flto

not ./mold -o /dev/null $t/a.o $t/b.o |&
  grep -q "b.o: don't know how to handle this LTO object file"
