#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CXX -o $t/a.o -c -xc++ -
#include <iostream>
int main() {
  std::cout << "Hello world" << std::endl;
}
EOF2

$CXX --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep 'Hello world'
