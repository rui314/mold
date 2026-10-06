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

# GNU ld's short aliases, and the options GNU ld accepts and ignores.

# -i is an alias for -r, so it makes a relocatable output.
$CC -B. -o $t/exe4 -Wl,-i $t/a.o
readelf -h $t/exe4 > $t/log4
grep 'Type:.*REL ' $t/log4

# -n does not page-align data, so the result is not run; -t traces inputs.
$CC -B. -o $t/exe5 -Wl,-n $t/a.o
$CC -B. -o $t/exe6 -Wl,-t $t/a.o > $t/log6
grep $t/a.o $t/log6

# A short option that GNU ld ignores, and one that forces common symbols
# to be defined.
$CC -B. -o $t/exe7 -Wl,-g $t/a.o
$CC -B. -o $t/exe8 -Wl,-d $t/a.o

# -a and -c take their value attached by an equal sign, so they must not
# be confused with the longer options that start with the same letter.
# (The built-in parser reads them as a separate word instead.)
$CC -B. -o $t/exe9 -Wl,-a=KEYWORD $t/a.o
$CC -B. -o $t/exe10 -Wl,-auxiliary -Wl,$t/a.o -Wl,-shared
$CC -B. -o $t/exe11 -Wl,--as-needed $t/a.o
$CC -B. -o $t/exe12 -Wl,--compress-debug-sections=zlib $t/a.o
$CC -B. -o $t/exe13 -Wl,-assert -Wl,KEYWORD $t/a.o
$CC -B. -o $t/exe14 -Wl,-Y -Wl,$t $t/a.o

# Vendor-specific spellings mold has no use for.
$CC -B. -o $t/exe15 -Wl,-Ur $t/a.o
$CC -B. -o $t/exe16 -Wl,-Qy $t/a.o
$CC -B. -o $t/exe17 -Wl,-A -Wl,x86-64 $t/a.o
$CC -B. -o $t/exe18 -Wl,-G -Wl,8 $t/a.o
$CC -B. -o $t/exe19 -Wl,-dT -Wl,$t/nosuchscript $t/a.o
$CC -B. -o $t/exe20 -Wl,-c=$t/nosuchscript $t/a.o

# The long names the short options stand for.
$CC -B. -o $t/exe21 -Wl,--architecture -Wl,x86-64 $t/a.o
$CC -B. -o $t/exe22 -Wl,--gpsize -Wl,8 $t/a.o
$CC -B. -o $t/exe23 -Wl,--mri-script -Wl,$t/nosuchscript $t/a.o
$CC -B. -o $t/exe24 -Wl,--default-script -Wl,$t/nosuchscript $t/a.o

# A name that merely starts like an option is still unknown.
not ./mold -auxiliaries |& grep 'unknown command line option: -auxiliaries'
not ./mold -a KEYWORD |& grep 'unknown command line option: -a'
