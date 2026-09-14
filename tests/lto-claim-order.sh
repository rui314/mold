#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = $(uname -m) ] || skip
echo 'int main() {}' | clang++ -B. -flto -o /dev/null -xc++ - >& /dev/null || skip

# LLVM's LTO expects to receive IR files in the command line order. If
# the module holding a non-prevailing copy of a COMDAT group arrives
# before the module holding the prevailing copy, the internal initializer
# of the losing copy becomes an undefined reference. A file named by a
# linker script is found after all command line files, so a.o below is
# such a late arrival unless IR files are claimed in the command line
# order.

cat <<EOF > $t/a.h
int f();
template <typename T> struct S { static int x; };
template <typename T> int S<T>::x = f();
EOF

cat <<EOF | clang++ -flto -c -o $t/a.o -xc++ -
#include "$t/a.h"
int f() { static int n; return ++n; }
int a() { return S<int>::x; }
EOF

cat <<EOF | clang++ -flto -c -o $t/b.o -xc++ -
#include "$t/a.h"
int b() { return S<int>::x; }
EOF

cat <<EOF | clang++ -c -o $t/c.o -xc++ -
#include <cstdio>
int a();
int b();
int main() { printf("%d %d\n", a(), b()); }
EOF

echo "INPUT($t/a.o)" > $t/script

clang++ -B. -flto -o $t/exe $t/script $t/b.o $t/c.o
$QEMU $t/exe | grep '^1 1$'
