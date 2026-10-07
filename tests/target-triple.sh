#!/bin/bash
source "$(dirname "$0")"/common.inc

# clang passes firmware's target to the linker as a triple, -target
# arm64-apple-firmware1.0.0, rather than as -arch and -platform_version.
# The triple overrides both wherever they are given, and names no SDK;
# a firmware triple needs no version.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
_start:
  ret
EOF

bv() { otool -l $1 | grep -A4 'cmd LC_BUILD_VERSION' | awk '$1 != "cmd" && $1 != "cmdsize" { printf "%s %s ", $1, $2 }'; }

$mold -target $ARCH-apple-firmware1.2.3 -e _start $t/a.o -o $t/exe -version_load_command
[ "$(bv $t/exe)" = 'platform 13 minos 1.2.3 sdk n/a ' ]

$mold -arch $ARCH -platform_version firmware 3.0 4.0 -target $ARCH-apple-firmware2.0 \
  -e _start $t/a.o -o $t/exe2 -version_load_command
[ "$(bv $t/exe2)" = 'platform 13 minos 2.0 sdk n/a ' ]

$mold -target $ARCH-apple-firmware -platform_version firmware 3.0 4.0 -e _start $t/a.o \
  -o $t/exe3 -version_load_command
[ "$(bv $t/exe3)" = 'platform 13 minos 0.0 sdk n/a ' ]

# So a firmware program links through the compiler driver - on arm64:
# clang makes an x86-64 one a simulator's, which ld-prime does not know.
# Xcode 26's clang knows no firmware and makes an ELF object for it.
echo 'int start() { return 0; }' | $CC -target $ARCH-apple-firmware -c -xc - -o $t/b.o \
  -Wno-incompatible-sysroot
if ! file $t/b.o | grep -q Mach-O; then
  :
elif [ $ARCH = arm64 ]; then
  $CC -target $ARCH-apple-firmware --ld-path=$mold -Wno-incompatible-sysroot -nostdlib \
    -e _start $t/b.o -o $t/exe4
  otool -hv $t/exe4 | grep -q EXECUTE
else
  not $CC -target $ARCH-apple-firmware --ld-path=$mold -Wno-incompatible-sysroot -nostdlib \
    -e _start $t/b.o -o $t/exe4 2> $t/log4
  grep -q "unknown OS in target triple 'x86_64-apple-firmware1.0.0-simulator'" $t/log4
fi

not $mold -target bogus -e _start $t/a.o -o $t/exe5 2> $t/log5
grep -q "missing dashes in target triple 'bogus'" $t/log5
not $mold -target $ARCH-apple-macos -e _start $t/a.o -o $t/exe5 2> $t/log5
grep -q "missing OS version in target triple '$ARCH-apple-macos'" $t/log5
not $mold -target $ARCH-apple-darwin -e _start $t/a.o -o $t/exe5 2> $t/log5
grep -q "unknown OS in target triple '$ARCH-apple-darwin'" $t/log5
