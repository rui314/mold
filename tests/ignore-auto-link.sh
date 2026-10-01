#!/bin/bash
source "$(dirname "$0")"/common.inc

# -ignore_auto_link turns auto-linking off: the objects' auto-link
# options and -add_linker_option's are neither read (nor warned about)
# nor acted on, and a -r output carries none of them.
cat <<EOF | $CC -o $t/foo.o -c -xc -
int foo(void) { return 3; }
EOF
rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/foo.o
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.linker_option "-lfoo"
.linker_option "bogus"
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
int foo(void);
int main() { return foo() != 3; }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -L$t 2> /dev/null
$t/exe
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -L$t -Wl,-ignore_auto_link 2> $t/log
not grep -q bogus $t/log
not $CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-add_linker_option,-lfoo \
  -Wl,-ignore_auto_link 2> /dev/null

$mold -r -arch $ARCH -o $t/r.o $t/a.o -add_linker_option -lbar -ignore_auto_link 2> $t/log
not grep -q bogus $t/log
otool -l $t/r.o > $t/lc
not grep -q LC_LINKER_OPTION $t/lc
