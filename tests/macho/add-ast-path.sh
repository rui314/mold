#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

echo hello > $t/foo.swiftmodule

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-add_ast_path,$t/foo.swiftmodule
nm -ap $t/exe | grep -q ' a .*foo.swiftmodule'
