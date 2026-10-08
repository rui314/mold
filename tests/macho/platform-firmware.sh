#!/bin/bash
source "$(dirname "$0")"/common.inc

# -platform_version's macOS and firmware are the point of the test.
on_simulator && skip

# -platform_version firmware (PLATFORM_FIRMWARE, 13) builds for no OS.
# An image of it is still one dyld loads unless -static or -preload says
# otherwise, but ld-prime takes firmware as newer than any OS release
# (chained fixups at version 1.0), needs no libSystem, signs nothing by
# default, and records no LC_BUILD_VERSION unless -version_load_command
# asks for it, -r output included.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
.p2align 2
_start:
  ret
.data
.p2align 3
_p: .quad _start
EOF

fw='-platform_version firmware 1.0 1.0'
cmds() { otool -l $1 | awk '$1 == "cmd" { printf "%s ", $2 }'; }

$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe
otool -hv $t/exe | grep -q ' EXECUTE .* NOUNDEFS DYLDLINK TWOLEVEL PIE$'
cmds $t/exe > $t/cmds
grep -q 'LC_DYLD_CHAINED_FIXUPS LC_DYLD_EXPORTS_TRIE ' $t/cmds
grep -q 'LC_UUID LC_SOURCE_VERSION LC_MAIN ' $t/cmds
not grep -q LC_BUILD_VERSION $t/cmds
not grep -q LC_CODE_SIGNATURE $t/cmds
not grep -q LC_LOAD_DYLIB $t/cmds

$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe2 -version_load_command
otool -l $t/exe2 | grep -A3 'cmd LC_BUILD_VERSION' > $t/bv2
grep -q 'platform 13' $t/bv2
grep -q 'minos 1.0' $t/bv2

# The platform goes by any case of its name or by its number.
$mold -arch $ARCH -platform_version Firmware 1.0 1.0 -e _start $t/a.o -o $t/exe3
$mold -arch $ARCH -platform_version 13 1.0 1.0 -e _start $t/a.o -o $t/exe4
not grep -q LC_BUILD_VERSION <(cmds $t/exe3)

# 128 bytes are left free after the load commands unless -headerpad
# says otherwise.
first() { otool -l $1 | awk '$1 == "offset" { print $2; exit }'; }
$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe5 -headerpad 0x80
[ "$(first $t/exe)" = "$(first $t/exe5)" ]
$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe6 -headerpad 0x20
[ "$(first $t/exe6)" = $(($(first $t/exe) - 96)) ]

# Objects of any platform link in (an iOS one here); only a firmware
# object built for a newer version gets a warning.
echo 'int foo() { return 0; }' | $CC -target $ARCH-apple-ios15.0 -c -xc - -o $t/ios.o \
  -Wno-incompatible-sysroot
$mold -arch $ARCH $fw -e _start $t/a.o $t/ios.o -o $t/exe7 2> $t/log7
not grep -q warning $t/log7

echo 'int bar() { return 0; }' | $CC -c -xc - -o $t/fw.o
mark_firmware $t/fw.o
$mold -arch $ARCH $fw -e _start $t/a.o $t/fw.o -o $t/exe8 2> $t/log8
not grep -q warning $t/log8
$mold -arch $ARCH -platform_version firmware 0.5 0.5 -e _start $t/a.o $t/fw.o -o $t/exe8 2> $t/log8
grep -q "was built for newer 'firmware' version (1.0) than being linked (0.5)" $t/log8
echo 'int main() { return 0; }' | $CC -c -xc - -o $t/main.o
not $CC --ld-path=$mold $t/main.o $t/fw.o -o $t/exe9 2> $t/log9
grep -q "building for 'macOS', but linking in object file (.*fw.o) built for 'firmware'" $t/log9

# A dylib for another platform links in with a warning.
echo 'int baz() { return 0; }' | $CC --ld-path=$mold -dynamiclib -xc - -o $t/libbaz.dylib
$mold -arch $ARCH $fw -e _start $t/a.o $t/libbaz.dylib -o $t/exe10 2> $t/log10
grep -q "building for 'firmware', but linking in dylib (.*libbaz.dylib) built for 'macOS'" $t/log10

# A -r output of firmware objects has no LC_BUILD_VERSION either.
$mold -arch $ARCH -r $t/fw.o -o $t/c.o
not grep -q LC_BUILD_VERSION <(cmds $t/c.o)
$mold -arch $ARCH -r $t/fw.o -o $t/d.o -version_load_command
otool -l $t/d.o | grep -A2 'cmd LC_BUILD_VERSION' | grep -q 'platform 13'

# Firmware may choose its own layout. In an image dyld loads,
# __DATA_CONST is a standard segment, which goes before __DATA where
# the list names that alone.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.text
.globl _start
_start: ret
.section __DATA,__const
.p2align 3
.quad _start
.data
.quad 1
.section __FOO,__foo
.quad 2
EOF
$mold -arch $ARCH $fw -e _start $t/e.o -o $t/exe11 -segment_order __DATA:__FOO 2> $t/log11
grep -q -- '-segment_order lists __DATA, but not __DATA_CONST, assuming standard order' $t/log11
segs() { otool -l $1 | awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }'; }
[ "$(segs $t/exe11)" = '__PAGEZERO __TEXT __DATA_CONST __DATA __FOO __LINKEDIT ' ]
$mold -arch $ARCH $fw -e _start $t/e.o -o $t/exe12 -section_order __TEXT __text

# ld-prime deprecates -flat_namespace off macOS, and allows relocations
# in read-only segments in firmware.
$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe13 -flat_namespace 2> $t/log13
grep -q -- '-flat_namespace is deprecated on firmware' $t/log13
$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe14 -read_only_relocs suppress 2> $t/log14
not grep -q warning $t/log14
$CC --ld-path=$mold $t/main.o -o $t/exe15 -Wl,-read_only_relocs,suppress 2> $t/log15
grep -q -- '-read_only_relocs relocs cannot be used in this configuration' $t/log15
not $mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe16 -read_only_relocs bogus 2> $t/log16
grep -q -- '-read_only_relocs invalid option (warning | error | suppress)' $t/log16
# (warn is warning.)
$mold -arch $ARCH $fw -e _start $t/a.o -o $t/exe17 -read_only_relocs warn
