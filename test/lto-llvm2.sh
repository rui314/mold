#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = $(uname -m) ] || skip

echo 'int main() {}' | clang -B. -flto -o /dev/null -xc - >& /dev/null || skip

cat <<EOF | clang -flto -c -o $t/a.o -xc -
int main() {}
EOF

clang -B. -o $t/exe1 -flto $t/a.o -Wl,-mllvm,-pass-remarks=.
clang -B. -o $t/exe2 -flto $t/a.o -Wl,-plugin-opt,-pass-remarks=.

not clang -B. -o $t/exe3 -flto $t/a.o -Wl,-mllvm,--no-such-option |&
  grep -i 'Unknown command line argument'

not clang -B. -o $t/exe4 -flto $t/a.o -Wl,-plugin-opt,--no-such-option |&
  grep -i 'Unknown command line argument'
