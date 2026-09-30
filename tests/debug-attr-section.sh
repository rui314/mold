#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime drops the sections of the __DWARF and __LD segments, whatever
# their flags, but copies a section with S_ATTR_DEBUG anywhere else like
# any other: into a final image without the attribute, into a -r output
# with it.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__foo,regular,debug
.globl _foo
_foo: .quad 0x1234
.section __DWARF,__bar
.globl _bar
_bar: .quad 0x5678
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
extern long foo;
int main() { return foo != 0x1234; }
EOF

flags() {
  otool -l $1 | awk -v g=$2 -v s=$3 '$1 == "sectname" { n = $2 }
    $1 == "segname" { m = $2 } n == s && m == g && $1 == "flags" { print $2; exit }'
}

[ "$(flags $t/a.o __DATA __foo)" = 0x02000000 ]
[ "$(flags $t/a.o __DWARF __bar)" = 0x00000000 ]

$CC --ld-path=$mold -o $t/exe $t/a.o $t/main.o
[ "$(flags $t/exe __DATA __foo)" = 0x00000000 ]
otool -l $t/exe > $t/lc
not grep -q __DWARF $t/lc
$t/exe

$mold -arch $ARCH -r -o $t/r.o $t/a.o
[ "$(flags $t/r.o __DATA __foo)" = 0x02000000 ]
otool -l $t/r.o > $t/lc-r
not grep -q __DWARF $t/lc-r
nm $t/r.o > $t/syms-r
grep -q ' _foo$' $t/syms-r
not grep -q ' _bar$' $t/syms-r
