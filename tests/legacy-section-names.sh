#!/bin/bash
source "$(dirname "$0")"/common.inc

# Old compilers put coalesced (weak) code and data in sections of their
# own, which ld-prime gives the usual names: __TEXT,__textcoal_nt is
# __text, __const_coal (in __TEXT, __DATA or __DATA_CONST) __const and
# __datacoal_nt (in __DATA or __DATA_DIRTY) __data, in a final image and
# a -r output alike, and a boundary symbol's section too. It does so in
# place of -rename_section: one naming the old name renames the section
# instead, and -rename_segment moves it on. The __DATA_CONST move
# follows the old name, so __DATA,__const_coal stays in __DATA.
cat <<EOF | $CC -o $t/a.o -c -xassembler - 2> /dev/null
.section __TEXT,__textcoal_nt
.globl _t
_t: .quad 1
.section __TEXT,__const_coal
.quad 2
.section __DATA,__const_coal
.quad 3
.section __DATA,__datacoal_nt
.quad 4
.section __DATA,__const
.quad 5
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
extern char start __asm("section\$start\$__DATA\$__datacoal_nt");
int main() { return &start == 0; }
EOF
echo 'int main() { return 0; }' | $CC -o $t/main2.o -c -xc -

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { print $2 "," s; s = "" }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o $t/main.o
sects $t/exe > $t/sects
not grep -q coal $t/sects
grep -qx '__TEXT,__const' $t/sects
grep -qx '__DATA_CONST,__const' $t/sects
grep -qx '__DATA,__const' $t/sects
grep -qx '__DATA,__data' $t/sects
nm -m $t/exe > $t/syms
grep -q '(__TEXT,__text) external _t$' $t/syms
$t/exe

$mold -arch $ARCH -r -o $t/r.o $t/a.o
sects $t/r.o > $t/sects-r
not grep -q coal $t/sects-r
grep -qx '__TEXT,__const' $t/sects-r
grep -qx '__DATA,__data' $t/sects-r
nm -m $t/r.o > $t/syms-r
grep -q '(__TEXT,__text) external .*_t$' $t/syms-r

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/main2.o \
  -Wl,-rename_section,__TEXT,__textcoal_nt,__TEXT,__foo \
  -Wl,-rename_section,__DATA,__const_coal,__DATA,__bar \
  -Wl,-rename_segment,__DATA,__FOO
sects $t/exe2 > $t/sects2
grep -qx '__TEXT,__foo' $t/sects2
grep -qx '__FOO,__bar' $t/sects2
grep -qx '__FOO,__data' $t/sects2
not grep -q coal $t/sects2
