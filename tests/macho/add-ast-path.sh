#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

echo hello > $t/foo.swiftmodule

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-add_ast_path,$t/foo.swiftmodule
nm -ap $t/exe | grep ' a .*foo.swiftmodule'

# -S, which drops the debugger's notes, drops the paths too.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-add_ast_path,$t/foo.swiftmodule -Wl,-S
nm -ap $t/exe2 > $t/log2
not grep -q swiftmodule $t/log2

# A -r output lists them after the local symbols, in the order given,
# with or without debug notes.
cat <<EOF2 | $CC -o $t/b.o -c -xc -
static int s(void) { return 1; }
int f(void) { return s(); }
EOF2
$mold -arch $ARCH -r -o $t/c.o $t/b.o -add_ast_path /x/one.swiftmodule \
  -add_ast_path /x/two.swiftmodule
nm -ap $t/c.o > $t/log3
grep -A2 ' t _s$' $t/log3 > $t/after
grep -q ' a /x/one.swiftmodule' $t/after
tail -1 $t/after | grep -q ' a /x/two.swiftmodule'
