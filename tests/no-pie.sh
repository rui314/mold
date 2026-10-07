#!/bin/bash
source "$(dirname "$0")"/common.inc

# An executable is position independent (MH_PIE) unless -no_pie says
# otherwise. arm64 macOS runs PIE executables only, so ld-prime ignores
# -no_pie there with a warning, and it deprecates -no_pie from the OS
# versions that default to chained fixups (macOS 12 on arm64, 13 on
# x86-64). A dylib is never MH_PIE, and -no_pie on it says nothing.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -

flags() { otool -hv $1 | tail -1; }

if [ $ARCH = arm64 ]; then old=11.0; new=12.0; else old=12.0; new=13.0; fi

$CC --ld-path=$mold -o $t/exe $t/a.o -mmacosx-version-min=$new
flags $t/exe | grep -q ' PIE'

$CC --ld-path=$mold -o $t/exe1 $t/a.o -mmacosx-version-min=$old -Wl,-no_pie 2> $t/log1
$RUN $t/exe1
not grep -q 'deprecated' $t/log1

$CC --ld-path=$mold -o $t/exe2 $t/a.o -mmacosx-version-min=$new -Wl,-no_pie 2> $t/log2
$RUN $t/exe2
grep -q -- '-no_pie is deprecated when targeting new OS versions' $t/log2

if [ $ARCH = arm64 ]; then
  flags $t/exe1 | grep -q ' PIE'
  flags $t/exe2 | grep -q ' PIE'
  grep -q -- '-no_pie ignored for arm64' $t/log1
  grep -q -- '-no_pie ignored for arm64' $t/log2
else
  flags $t/exe1 > $t/flags1
  not grep -q ' PIE' $t/flags1
  flags $t/exe2 > $t/flags2
  not grep -q ' PIE' $t/flags2
fi

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -mmacosx-version-min=$new -Wl,-no_pie 2> $t/log3
not grep -q -- '-no_pie' $t/log3

# The last of -pie and -no_pie wins, with a warning for one that turns
# the other around.
$CC --ld-path=$mold -o $t/exe4 $t/a.o -mmacosx-version-min=$old \
  -Wl,-pie,-pie,-no_pie,-no_pie,-pie 2> $t/log4
flags $t/exe4 | grep -q ' PIE'
[ "$(grep -c -- '-no_pie overriding previous -pie' $t/log4)" = 1 ]
[ "$(grep -c -- '-pie overriding previous -no_pie' $t/log4)" = 1 ]
$CC --ld-path=$mold -o $t/exe4 $t/a.o -mmacosx-version-min=$old -Wl,-w,-pie,-no_pie \
  2> $t/log5
not grep -q overriding $t/log5

# ld64 made an x86-64 executable PIE by default from macOS 10.6 on: one
# for an older macOS is PIE only with -pie.
if [ $ARCH = x86_64 ]; then
  $CC --ld-path=$mold -o $t/exe6 $t/a.o -mmacosx-version-min=10.5 2> /dev/null
  $RUN $t/exe6
  flags $t/exe6 > $t/flags6
  not grep -q ' PIE' $t/flags6
  $CC --ld-path=$mold -o $t/exe7 $t/a.o -mmacosx-version-min=10.5 -Wl,-pie 2> /dev/null
  $RUN $t/exe7
  flags $t/exe7 | grep -q ' PIE'
  $CC --ld-path=$mold -o $t/exe8 $t/a.o -mmacosx-version-min=10.6 2> /dev/null
  flags $t/exe8 | grep -q ' PIE'
fi
