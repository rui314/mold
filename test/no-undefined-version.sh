#!/bin/bash
. $(dirname $0)/common.inc

echo 'ver_x { global: foo; };' > $t/a.ver

cat <<EOF | $CC -o $t/b.o -c -xc -
int main() {}
EOF

$CC -B. -o $t/exe -Wl,--version-script,$t/a.ver $t/b.o |&
  grep -F 'a.ver: cannot assign version `ver_x` to symbol `foo`: symbol not found'
