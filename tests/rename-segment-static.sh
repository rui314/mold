#!/bin/bash
source "$(dirname "$0")"/common.inc

# XNU moves its code out of __TEXT with -rename_section: the section
# goes, with no empty __TEXT,__text left behind, and __TEXT_EXEC is
# executable. In a -static image -rename_segment __TEXT moves the mach
# header too, and ld-prime refuses the image unless -segment_order
# puts that segment first after __PAGEZERO. On x86-64 a -static
# image's synthesized __eh_frame is renamed like the rest.
cat <<EOF | $CC -o $t/a.o -c -xc -
const char *s = "hello";
int f(int x) { return x + 1; }
int main() { return f(s[0]); }
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { print $2 "," s; s = "" }'
}
segs() { otool -l $1 | awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }'; }

$mold -arch $ARCH -static -e _main -o $t/exe $t/a.o -rename_section __TEXT __text __TEXT_EXEC __text
sects $t/exe > $t/sects
grep -qx '__TEXT_EXEC,__text' $t/sects
not grep -q '__TEXT,__text' $t/sects
otool -l $t/exe | grep -A6 'segname __TEXT_EXEC' | grep -q 'initprot 0x00000005'

not $mold -arch $ARCH -static -e _main -o $t/exe2 $t/a.o -rename_segment __TEXT __FOO 2> $t/log2
grep -q 'Invalid -segment_order, __TEXT must be the first segment after zero page' $t/log2

$mold -arch $ARCH -static -e _main -o $t/exe3 $t/a.o -rename_segment __TEXT __FOO \
  -segment_order __FOO:__DATA
[ "$(segs $t/exe3)" = '__PAGEZERO __FOO __DATA __LINKEDIT ' ]
otool -l $t/exe3 | grep -A4 'segname __FOO' | grep -q 'fileoff 0'

if [ $ARCH = x86_64 ]; then
  $mold -arch $ARCH -static -e _main -o $t/exe4 $t/a.o -rename_section __TEXT __eh_frame __FOO __eh
  grep -qx '__FOO,__eh' <(sects $t/exe4)
  not grep -q '__eh_frame' <(sects $t/exe4)
fi
