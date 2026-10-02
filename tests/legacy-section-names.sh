#!/bin/bash
source "$(dirname "$0")"/common.inc

# Old compilers put coalesced (weak) code and data in sections of their
# own: __TEXT,__textcoal_nt, __const_coal and __datacoal_nt. They come
# out under their own names, as plain data here (a coalesced type
# directs the linker alone), in a final image and a -r output alike,
# and a boundary symbol names them as such; -rename_section and
# -rename_segment rename them like any other section. (ld-prime gives
# them the modern names __text, __const and __data in place of a
# -rename_section.)
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
int main() { return start != 4; }
EOF
echo 'int main() { return 0; }' | $CC -o $t/main2.o -c -xc -

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { g = $2 }
    $1 == "flags" && s != "" { print g "," s, $2; s = "" }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o $t/main.o
sects $t/exe > $t/sects
grep -qx '__TEXT,__textcoal_nt 0x00000000' $t/sects
grep -qx '__TEXT,__const_coal 0x00000000' $t/sects
grep -qx '__DATA,__const_coal 0x00000000' $t/sects
grep -qx '__DATA,__datacoal_nt 0x00000000' $t/sects
grep -qx '__DATA_CONST,__const 0x00000000' $t/sects
nm -m $t/exe > $t/syms
grep -q '(__TEXT,__textcoal_nt) external _t$' $t/syms
$t/exe

$mold -arch $ARCH -r -o $t/r.o $t/a.o
sects $t/r.o > $t/sects-r
grep -q '^__TEXT,__textcoal_nt ' $t/sects-r
grep -q '^__DATA,__datacoal_nt ' $t/sects-r
$CC --ld-path=$mold -o $t/exe-r $t/r.o $t/main.o
$t/exe-r

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/main2.o \
  -Wl,-rename_section,__TEXT,__textcoal_nt,__TEXT,__foo \
  -Wl,-rename_section,__DATA,__const_coal,__DATA,__bar \
  -Wl,-rename_segment,__DATA,__FOO
sects $t/exe2 > $t/sects2
grep -q '^__TEXT,__foo ' $t/sects2
grep -q '^__FOO,__bar ' $t/sects2
grep -q '^__FOO,__datacoal_nt ' $t/sects2
