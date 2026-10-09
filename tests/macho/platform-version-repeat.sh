#!/bin/bash
source "$(dirname "$0")"/common.inc

# -platform_version's macOS and firmware are the point of the test.
on_simulator && skip

# The last option naming the deployment target wins. ld-prime warns, as
# it reads the option, about another minimum version for the platform,
# and about firmware replacing macOS; macOS replacing firmware is an
# error.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
EOF

link() { $mold -arch $ARCH -syslibroot "$SDK" $t/a.o -e _main "$@"; }

link -lSystem -platform_version macos 14.0 15.0 -macos_version_min 13.0.1 \
  -platform_version macos 13.0.1 15.0 -o $t/exe1 2> $t/log1
grep -q 'passed two min versions (14.0, 13.0.1) for platform macOS. Using 13.0.1.' $t/log1
otool -l $t/exe1 | grep -A4 LC_BUILD_VERSION | grep -q 'minos 13.0.1'

# A -w before the options silences the warnings, which are given as
# the options are read.
link -lSystem -w -platform_version macos 14.0 15.0 -macos_version_min 13.0 -o $t/exe2 \
  2> $t/log2
not grep -q 'passed two' $t/log2

link -platform_version macos 14.0 14.0 -platform_version firmware 13.0 15.0 -o $t/exe3 \
  2> $t/log3
grep -q 'conflicting -platform_version platform: macOS, using: firmware' $t/log3

not link -platform_version firmware 13.0 13.0 -platform_version macos 13.0 15.0 \
  -o $t/exe4 2> $t/log4
grep -q 'incompatible platforms: firmware - macOS' $t/log4
