#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CXX -o $t/a.o -c -xc++ -
#include <iostream>
struct T { T() { std::cout << "foo "; } };
T x;
EOF2

cat <<EOF2 | $CXX -o $t/b.o -c -xc++ -
#include <iostream>
struct T { T() { std::cout << "foo "; } };
T y;
EOF2

cat <<EOF2 | $CXX -o $t/c.o -c -xc++ -
#include <iostream>
int main() { std::cout << "bar\n"; }
EOF2

$CXX --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$t/exe | grep '^foo foo bar$'
