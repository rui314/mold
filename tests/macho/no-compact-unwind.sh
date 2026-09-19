#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CXX -c -o $t/a.o -xc++ -mmacosx-version-min=10.1 -
int main() {
  try {
    throw 0;
  } catch (int x) {
    return x;
  }
  return 1;
}
EOF

$CXX --ld-path=$mold -o $t/exe $t/a.o
$t/exe
