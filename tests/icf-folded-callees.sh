#!/bin/bash
source "$(dirname "$0")"/common.inc

# Two identical functions that call two other identical functions fold
# together once their callees do. A debug build checks each folded
# class against its leader, and used to see different callees there
# and panic.
cat <<EOF | $CXX -o $t/a.o -c -xc++ - -O0
template <int N> __attribute__((noinline)) int leaf(int x) { return x + 1; }
template <int N> __attribute__((noinline)) int mid(int x) { return leaf<N>(x) * 2; }
int main() { return mid<1>(1) + mid<2>(2) != 10; }
EOF
$CXX --ld-path=$mold -o $t/exe $t/a.o
$t/exe
