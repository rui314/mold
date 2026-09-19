#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CXX -o $t/a.o -c -xc++ -
#include <cstdio>
void thrower() { throw 42; }
int main() {
  try {
    thrower();
  } catch (int e) {
    printf("caught %d\n", e);
  }
}
EOF2

$CXX --ld-path=$mold -o $t/exe $t/a.o
$t/exe | grep 'caught 42'
