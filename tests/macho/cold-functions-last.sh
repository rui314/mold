#!/bin/bash
source "$(dirname "$0")"/common.inc

# Clang splits a function's rarely-run part into foo.cold.1 and marks
# both parts N_COLD_FUNC. ld64 lays cold atoms out after every other
# atom of their section, in final images and -r outputs alike, so hot
# code stays dense; we kept them in input order.
cat <<EOF2 | $CXX -O2 -o $t/a.o -c -xc++ -
struct S { virtual ~S(); virtual int g(); };
S::~S() {}
int S::g() { try { throw 1; } catch (int) { return 2; } }
EOF2
cat <<EOF2 | $CXX -O2 -o $t/b.o -c -xc++ -
struct S { virtual ~S(); virtual int g(); };
__attribute__((noinline)) int after(int x) { return x + 1; }
int main() { S s; return s.g() + after(0) - 3; }
EOF2
nm -m $t/a.o | grep -q 'cold func'

$CXX --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe
nm -n $t/exe | grep -E ' [Tt] ' | awk '{print $3}' > $t/order
# The cold S::g comes after b.o's functions.
[ "$(tail -1 $t/order)" = __ZN1S1gEv ]
grep -q '^__Z5afteri$' $t/order

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
nm -n $t/r.o | grep -E ' [Tt] ' | awk '{print $3}' > $t/rorder
[ "$(tail -1 $t/rorder)" = __ZN1S1gEv ]
