#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

# These spellings need the winnow-args parser (--features winnow-args);
# the built-in parser does not take them yet.
./mold -max-cache-size=1 $t/a.o > $t/probe 2>&1 || true
if ! grep -q 'unknown -m argument' $t/probe; then
  skip
fi

# GNU ld's dash rules: a long option spelled with one dash is read as the
# long option, not as a short option with the rest of its name as a value.

# -entry=main is --entry=main, not -e ntry=main: the entry point is main.
entry_of() {
  readelf -h $1 | awk '/Entry point address/ {print $4}'
}
main_of() {
  printf '0x%x' "$(($(nm $1 | awk '$3 == "main" {print "16#" $1}')))"
}

$CC -B. -o $t/exe1 -Wl,-entry=main $t/a.o
test "$(entry_of $t/exe1)" = "$(main_of $t/exe1)"

# -emain is -e main.
$CC -B. -o $t/exe2 -Wl,-emain $t/a.o
test "$(entry_of $t/exe2)" = "$(main_of $t/exe2)"

# -Ttext=0x1000 sets the text section address, as --Ttext=0x1000 does.
$CC -B. -o $t/exe3 -Wl,-Ttext=0x1000 $t/a.o
readelf -SW $t/exe3 | grep -E '\.text\s+PROGBITS\s+0000000000001000' > /dev/null

# Where GNU ld reads the single-dash spelling as a short option with an
# attached value: -export-dynamic-symbol is -e xport-dynamic-symbol, so foo
# is an input file, and -max-cache-size=1 is -m ax-cache-size=1.
not ./mold -export-dynamic-symbol foo $t/a.o |& grep 'cannot open foo'
not ./mold -max-cache-size=1 $t/a.o |& grep 'unknown -m argument: ax-cache-size=1'

# -omagic is -o magic, as in GNU ld.
(cd $t && $OLDPWD/mold -omagic a.o)
test -f $t/magic
