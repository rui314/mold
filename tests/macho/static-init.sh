#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CXX -o $t/a.o -c -xc++ -
#include <cstdio>
struct S { S() { printf("ctor\n"); } };
S s;
int main() { printf("main\n"); }
EOF2

$CXX --ld-path=$mold -o $t/exe $t/a.o
$t/exe > $t/log
head -1 $t/log | grep ctor
tail -1 $t/log | grep main
