#!/bin/bash
source "$(dirname "$0")"/common.inc

# -rename_section and -rename_segment apply to a -r output too, to the
# input sections as they came (no __DATA_CONST move) and to the merged
# __objc_imageinfo record.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.zerofill __AAA,__zz,_z,64,4
.text
.globl _main
_main:
  ret
.section __DATA,__const
.quad 1
.section __AAA,__a
.quad 2
.data
.quad 3
.section __DATA,__objc_imageinfo,regular,no_dead_strip
.long 0, 64
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { print $2 "," s; s = "" }'
}

$mold -r -arch $ARCH -o $t/b.o $t/a.o -rename_section __DATA __const __FOO __bar \
  -rename_section __DATA_CONST __const __BAD __bad -rename_segment __AAA __BBB \
  -rename_section __DATA __objc_imageinfo __FOO __ii -rename_segment __DATA __CCC
sects $t/b.o > $t/sects
grep -qx '__FOO,__bar' $t/sects
grep -qx '__BBB,__a' $t/sects
grep -qx '__FOO,__ii' $t/sects
grep -qx '__CCC,__data' $t/sects
grep -qx '__BBB,__zz' $t/sects
not grep -q '__BAD\|__AAA\|__DATA' $t/sects

# The output links.
echo 'int main() { return 0; }' | $CC -o $t/c.o -c -xc -
$mold -r -arch $ARCH -o $t/d.o $t/c.o -rename_section __TEXT __text __TEXT __text2
$CC --ld-path=$mold -o $t/exe $t/d.o
$t/exe
