#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {
  return 0;
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o
$t/exe
