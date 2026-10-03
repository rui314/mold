#!/bin/bash
source "$(dirname "$0")"/common.inc

# -platform_version takes a platform by name, in any case, or by its
# number, and a version made of decimal numbers that must fit
# LC_BUILD_VERSION's 16.8.8 bits. mold links for macOS and firmware.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
EOF

sdk=$(xcrun --show-sdk-path)
link() { $mold -arch $ARCH -syslibroot "$sdk" -lSystem $t/a.o "$@"; }
bv() { otool -l $1 | grep -A4 'cmd LC_BUILD_VERSION' | awk '$1 == "platform" || $1 == "minos" { printf "%s %s ", $1, $2 }'; }

link -platform_version MacOS 14.0 14.0 -o $t/exe1
[ "$(bv $t/exe1)" = 'platform 1 minos 14.0 ' ]
link -platform_version 01 14..1 14.0 -o $t/exe2
[ "$(bv $t/exe2)" = 'platform 1 minos 14.0.1 ' ]

for p in foo 0 31 iossimulator macos-simulator; do
  not link -platform_version $p 14.0 14.0 -o $t/exe3 2> $t/log3
  grep -q "platform: $p$" $t/log3
done
# (ld-prime takes iOS, and refuses the macOS object file.)
for p in ios 2; do
  not link -platform_version $p 14.0 14.0 -o $t/exe3
done

not link -platform_version macos 14.x 14.0 -o $t/exe4 2> $t/log4
grep -q -- "-platform_version: malformed 32-bit xxxx.yy.zz version number: '14.x'" $t/log4
not link -platform_version macos 14.0 14. -o $t/exe4 2> $t/log4
grep -q -- "-platform_version: malformed 32-bit xxxx.yy.zz version number: '14.'" $t/log4
not link -platform_version macos 14.256 14.0 -o $t/exe5 2> $t/log5
grep -q -- "-platform_version: malformed version number '14.256' cannot fit in 32-bit xxxx.yy.zz" $t/log5
not link -platform_version macos 14.0.0.1 14.0 -o $t/exe5 2> $t/log5
grep -q -- "-platform_version: malformed version number '14.0.0.1' cannot fit in 32-bit xxxx.yy.zz" $t/log5
not link -macos_version_min abc -o $t/exe6 2> $t/log6
grep -q -- "-macos_version_min: malformed 32-bit xxxx.yy.zz version number: 'abc'" $t/log6
not link -target $ARCH-apple-macos70000 -o $t/exe7 2> $t/log7
grep -q "[^-]malformed version number '70000' cannot fit in 32-bit xxxx.yy.zz" $t/log7
link -target $ARCH-apple-MacOS14.0 -o $t/exe8
[ "$(bv $t/exe8)" = 'platform 1 minos 14.0 ' ]

# A dylib's versions are truncated to fit instead, each number to its
# most and the numbers past the third dropped.
link -platform_version macos 14.0 14.0 -dylib -o $t/lib.dylib \
  -current_version 70000.1 -compatibility_version 1.2.3.4 2> $t/log9
grep -q -- '-current_version to fit in 32-bit space used by old mach-o format' $t/log9
grep -q -- '-compatibility_version to fit in 32-bit space used by old mach-o format' $t/log9
otool -l $t/lib.dylib | grep -A5 LC_ID_DYLIB > $t/id
grep -q 'current version 65535.1.0' $t/id
grep -q 'compatibility version 1.2.3' $t/id
not link -platform_version macos 14.0 14.0 -dylib -o $t/lib2.dylib -current_version 1.x \
  2> $t/log10
grep -q -- "-current_version: malformed 32-bit xxxx.yy.zz version number: '1.x'" $t/log10
