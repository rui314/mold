#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-headerpad,0x1000
$t/exe
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-headerpad_max_install_names
$t/exe
