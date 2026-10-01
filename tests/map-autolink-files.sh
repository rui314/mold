#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime's -map lists the files auto-link options name in the order
# it acts on the options, sorted: dylibs, and the members it loads from
# archives, where the archive comes. Before macOS 15, an $ld$previous
# directive gives libswift_Builtin_float, which libswiftDarwin merges,
# libswiftDarwin's install name: the -map lists the file a symbol
# binds to, libswift_Builtin_float where its option names it, or after
# the auto-linked files if none does, and lists libswiftDarwin only if
# a symbol of its own binds. A library all of whose bound exports moved
# to an older one comes where it is named all the same.
sdk=$(xcrun --show-sdk-path)
[ -f $sdk/usr/lib/swift/libswift_Builtin_float.tbd ] || skip

tbd() {
  cat <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '$1'
current-version: 1
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ $2 ]
...
EOF
}
tbd /aaa/libaaa.dylib _aaa > $t/libaaa.tbd
tbd /zzz/libzzz.dylib _zzz > $t/libzzz.tbd
tbd /ooo/libmoved.dylib "_mv, '\$ld\$previous\$/ooo/libold.dylib\$\$1\$10.15\$15.0\$_mv\$'" > $t/libmoved.tbd
echo 'int mid(void) { return 0; }' | $CC -o $t/mid.o -c -xc -
rm -f $t/libmid.a
ar rcs $t/libmid.a $t/mid.o

cat <<'EOF' | $CC -mmacos-version-min=14.0 -o $t/a.o -c -xc -
__asm__(".linker_option \"-lzzz\"");
__asm__(".linker_option \"-lswift_Builtin_float\"");
__asm__(".linker_option \"-lswiftDarwin\"");
__asm__(".linker_option \"-lmoved\"");
__asm__(".linker_option \"-lmid\"");
__asm__(".linker_option \"-laaa\"");
void aaa(void), zzz(void), mv(void);
int mid(void);
void dbl(void) __asm__("_$s6Darwin11DBL_EPSILONSdvg");
int main() { aaa(); zzz(); mv(); dbl(); return mid(); }
EOF

$CC --ld-path=$mold -mmacos-version-min=14.0 -o $t/exe $t/a.o -L$t -L$sdk/usr/lib/swift \
  -Wl,-map,$t/map
sed -n '/^# Object files:/,/^# Sections:/p' $t/map | grep '^\[' | grep -o '[^/]*$' |
  tr '\n' ' ' > $t/files
[ "$(cat $t/files)" = "[  0] linker synthesized a.o libSystem.tbd libaaa.tbd \
libmid.a(mid.o) libmoved.tbd libswift_Builtin_float.tbd libzzz.tbd " ]
