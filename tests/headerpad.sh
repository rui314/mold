#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime leaves free space after the load commands so that tools can
# add or grow commands in place: -headerpad, at least 32 bytes in a
# final image, or with -headerpad_max_install_names room for each dylib
# command to grow to MAXPATHLEN (1024). It places the sections after an
# estimate of the load commands rather than their final size, so the
# space grows by what the estimate over-counts: a 28-byte header for
# each dependency (an 8-byte step for /usr/lib/libz.1.dylib), and 16
# bytes for classic dyld info, 32 for x86-64 chained fixups or -static.
# A -r output gets -headerpad as given, 32 by default.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

# Checks that the first section starts `want` bytes past the load
# commands, rounded up to its alignment unless in a -r output.
gap() {
  local end=$((32 + $(otool -h $1 | tail -1 | awk '{print $7}')))
  local first=$(otool -l $1 | awk '$1 == "offset" && $2 > 0 { print $2; exit }')
  local align=1
  if [ "$3" != r ]; then
    align=$((1 << $(otool -l $1 | awk '$1 == "align" { sub(/2\^/, "", $2); print $2; exit }')))
  fi
  [ $first = $(( (end + $2 + align - 1) / align * align )) ]
}

if [ $ARCH = arm64 ]; then chained=0; else chained=32; fi

$CC --ld-path=$mold -o $t/exe $t/a.o
gap $t/exe $((32 + chained))
$CC --ld-path=$mold -o $t/exe2 $t/a.o -mmacosx-version-min=11.0
gap $t/exe2 48
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-headerpad,0x100
gap $t/exe3 $((256 + chained))
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-headerpad,0x10
gap $t/exe4 $((32 + chained))
$CC --ld-path=$mold -o $t/exe5 $t/a.o -lz -Wl,-headerpad_max_install_names
gap $t/exe5 $((2048 + 8 + chained))
$t/exe5
$CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-headerpad,0x1000
gap $t/exe6 $((4096 + chained))
$t/exe6

$mold -arch $ARCH -static -e _main -o $t/static $t/a.o
gap $t/static 64

$mold -arch $ARCH -r -o $t/r.o $t/a.o
gap $t/r.o 32 r
$mold -arch $ARCH -r -headerpad 0x10 -o $t/r2.o $t/a.o
gap $t/r2.o 16 r
