#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime leaves free space after the load commands so that tools can
# add or grow commands in place: -headerpad, at least 32 bytes in an
# image dyld loads (with a warning if -headerpad asks for less), or
# with -headerpad_max_install_names room for each dylib command to grow
# to MAXPATHLEN (1024). It places the sections after an estimate of the
# load commands rather than their final size, so the space grows by
# what the estimate over-counts: a 28-byte header for each dependency
# (an 8-byte step for /usr/lib/libz.1.dylib), 16 bytes for classic dyld
# info, 32 for x86-64 chained fixups or -static, and 80 for the header
# of a section it gives a static executable's stack. A -r output, which
# no tool adds commands to, gets none: its contents start right after
# its load commands, aligned for its sections. (ld-prime gives it
# -headerpad, 32 by default.)
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

# Checks that the first section starts `want` bytes past the load
# commands, rounded up to its alignment, or in a -r output (r) to the
# largest section alignment.
gap() {
  local end=$((32 + $(otool -h $1 | tail -1 | awk '{print $7}')))
  local first=$(otool -l $1 | awk '$1 == "offset" && $2 > 0 { print $2; exit }')
  local align
  if [ "$3" = r ]; then
    align=$((1 << $(otool -l $1 | awk '$1 == "align" { sub(/2\^/, "", $2); if ($2 + 0 > m) m = $2 + 0 }
      END { print m + 0 }')))
  else
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
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-headerpad,0x10 2> $t/log4
gap $t/exe4 $((32 + chained))
grep -q -- '-headerpad 0x10 is too small, at least 32 bytes are required to reserve space for code signature' $t/log4
$CC --ld-path=$mold -o $t/exe5 $t/a.o -lz -Wl,-headerpad_max_install_names
gap $t/exe5 $((2048 + 8 + chained))
$t/exe5
$CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-headerpad,0x1000
gap $t/exe6 $((4096 + chained))
$t/exe6

$mold -arch $ARCH -static -e _main -o $t/static $t/a.o
gap $t/static 64
$mold -arch $ARCH -static -e _main -headerpad 0 -o $t/static2 $t/a.o 2> $t/log7
gap $t/static2 32
not grep -q 'too small' $t/log7
$mold -arch $ARCH -static -e _main -stack_size 0x8000 -o $t/static3 $t/a.o
gap $t/static3 $((64 + 80))

$mold -arch $ARCH -r -o $t/r.o $t/a.o
gap $t/r.o 0 r
$mold -arch $ARCH -r -headerpad 0x10 -o $t/r2.o $t/a.o
gap $t/r2.o 0 r
