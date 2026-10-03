#!/bin/bash
source "$(dirname "$0")"/common.inc

# A "$ld$..." directive that doesn't parse is passed over without a
# word: one with a version that isn't X[.Y[.Z]] in 16.8.8 bits (1.2.3.1,
# 70000, 14.0x, 10.), say. An $ld$previous symbol needs no final '$',
# and with no symbol field at all the directive renames the whole
# library. A stub may also give the compatibility version for an OS
# version ($ld$compatibility_version$os<ver>$<version>), which a dylib
# binary can't: there it is ignored, as any kind of directive it doesn't
# know.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { return foo(); }
EOF

# link <name> <directive>...: links against a stub exporting _foo and
# the directives, for macOS 14.0.
link() {
  name=$1
  shift
  syms=_foo
  for d in "$@"; do syms="$syms, '$d'"; done
  cat > $t/$name.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    /usr/lib/lib$name.dylib
current-version: 5
compatibility-version: 4
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ $syms ]
...
EOF
  $CC --ld-path=$mold -o $t/exe $t/a.o $t/$name.tbd -Wl,-platform_version,macos,14.0,14.0 \
    2> $t/log
  not grep -q warning $t/log
  otool -L $t/exe > $t/libs
}

link bad '$ld$previous$bad'
grep -q /usr/lib/libbad.dylib $t/libs

link nodollar '$ld$previous$/usr/lib/libold.dylib$$1$10.0$20.0$_foo'
grep -q /usr/lib/libold.dylib $t/libs

link nosym '$ld$previous$/usr/lib/libold.dylib$2.0$1$10.0$20.0'
grep -q '/usr/lib/libold.dylib (compatibility version 2.0.0, current version 2.0.0)' $t/libs

link badcompat '$ld$previous$/usr/lib/libold.dylib$1.2.3.1$1$10.0$20.0$_foo$'
grep -q /usr/lib/libbadcompat.dylib $t/libs

link badlo '$ld$previous$/usr/lib/libold.dylib$$1$10.$20.0$_foo$'
grep -q /usr/lib/libbadlo.dylib $t/libs

link badname '$ld$install_name$os14.0x$/usr/lib/libnew.dylib'
grep -q /usr/lib/libbadname.dylib $t/libs

link name '$ld$install_name$os14.0.0$/usr/lib/libnew.dylib'
grep -q /usr/lib/libnew.dylib $t/libs

link compat '$ld$compatibility_version$os14$3.0'
grep -q '/usr/lib/libcompat.dylib (compatibility version 3.0.0, current version 5.0.0)' $t/libs

# A dylib binary with directives of kinds ld-prime doesn't know there.
cat <<'EOF' | $CC -o $t/b.o -c -xc -
int foo(void) { return 1; }
__asm__(".globl \"$ld$compatibility_version$os14.0$3.0\"\n"
        "\"$ld$compatibility_version$os14.0$3.0\" = 0\n"
        ".globl \"$ld$bogus$x\"\n\"$ld$bogus$x\" = 0\n");
EOF
$CC -shared -o $t/libb.dylib $t/b.o -install_name /usr/lib/libb.dylib \
  -Wl,-compatibility_version,4
$CC --ld-path=$mold -o $t/exe $t/a.o $t/libb.dylib -Wl,-platform_version,macos,14.0,14.0 \
  2> $t/log
otool -L $t/exe | grep -q '/usr/lib/libb.dylib (compatibility version 4.0.0'
