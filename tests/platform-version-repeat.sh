#!/bin/bash
source "$(dirname "$0")"/common.inc

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

sdk=$(xcrun --show-sdk-path)
link() { $mold -arch $ARCH -syslibroot "$sdk" $t/a.o -e _main "$@"; }

link -lSystem -platform_version macos 14.0 15.0 -macos_version_min 13.0.1 \
  -platform_version macos 13.0.1 15.0 -o $t/exe1 2> $t/log1
grep -q 'passed two min versions (14.0, 13.0.1) for platform macOS. Using 13.0.1.' $t/log1
[ "$(grep -c 'passed two min versions' $t/log1)" = 1 ]
otool -l $t/exe1 | grep -A4 LC_BUILD_VERSION | grep -q 'minos 13.0.1'

# Only a -w before the option silences the warnings.
link -lSystem -w -platform_version macos 14.0 15.0 -macos_version_min 13.0 -o $t/exe2 \
  2> $t/log2
not grep -q 'passed two' $t/log2
link -lSystem -platform_version macos 14.0 15.0 -macos_version_min 13.0 -w -o $t/exe2 \
  2> $t/log2
grep -q 'passed two' $t/log2

link -platform_version macos 14.0 14.0 -platform_version firmware 13.0 15.0 -o $t/exe3 \
  2> $t/log3
grep -q 'conflicting -platform_version platform: macOS, using: firmware' $t/log3

not link -platform_version firmware 13.0 13.0 -platform_version macos 13.0 15.0 \
  -o $t/exe4 2> $t/log4
grep -q 'incompatible platforms: firmware - macOS' $t/log4
