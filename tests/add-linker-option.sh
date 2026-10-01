#!/bin/bash
source "$(dirname "$0")"/common.inc

# -add_linker_option gives an auto-link option as if an object had one:
# a final link loads what it names, and a -r output carries it with the
# objects' (libraries first, each kind sorted). ld-prime reads the words
# of every one in a row, as an object's, before any object's, and warns
# "in command line". An option with a space is split there only if its
# first word names a framework (has "framework" in it); any other is
# ignored with a warning as it is read.
cat <<EOF | $CC -o $t/foo.o -c -xc -
int foo(void) { return 3; }
EOF
rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/foo.o
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.linker_option "-lbbb"
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -Wl,-add_linker_option,-lfoo -Wl,-u,_foo
nm $t/exe | grep -q ' T _foo$'

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o -add_linker_option -lzzz \
  -add_linker_option '-framework Foo Bar' -add_linker_option '-needed_framework Baz' \
  -add_linker_option '-framework Foo Bar' 2> $t/log
[ ! -s $t/log ]
otool -l $t/r.o | grep -A4 LC_LINKER_OPTION | awk '$1 == "string" { $1 = $2 = ""; print }' |
  tr '\n' '|' > $t/opts
[ "$(cat $t/opts)" = '  -lbbb|  -lzzz|  -needed_framework|  Baz|  -framework|  Foo Bar|' ]

$mold -r -arch $ARCH -o $t/r.o $t/a.o -add_linker_option '-lfoo -lbar' \
  -add_linker_option bogus -add_linker_option '-weak-lfoo' -add_linker_option -framework 2> $t/log
grep -q "unknown linker option from -add_linker_option ignored, starting with: '-lfoo'" $t/log
grep -q "unknown linker option from object file ignored: 'bogus' in command line" $t/log
grep -q "unexpected linker option from object file ignored: '-weak-lfoo' in command line" $t/log
grep -q "malformed linker option from object file ignored: '-framework missing <path>', in command line" $t/log
not grep -q LC_LINKER_OPTION $t/r.o

not $mold -arch $ARCH -o $t/exe $t/a.o -add_linker_option 2> $t/log
grep -q -- '-add_linker_option missing <options>' $t/log
not $mold -arch $ARCH -o $t/exe $t/a.o -add_linker_option '' 2> $t/log
grep -q -- '-add_linker_option missing <options>' $t/log
