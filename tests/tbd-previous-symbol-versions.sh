#!/bin/bash
source "$(dirname "$0")"/common.inc

# Exports of two libraries move to one older library by per-symbol
# $ld$previous directives that give no version, so each binds to it at
# its own library's version. ld-prime gives the older library one load
# command, at the version of the first library whose moved exports
# bind: SwiftUI's is at SwiftUICore's version if only SwiftUICore's
# exports moved there bind, at DeveloperToolsSupport's if its do too.
lib() { # name version symbol
  cat <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '/usr/lib/lib$1.dylib'
current-version: $2
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ $3, '\$ld\$previous\$/usr/lib/libold.dylib\$\$1\$10.0\$14.0\$$3\$' ]
...
EOF
}
lib one 3 _one > $t/libone.tbd
lib two 5 _two > $t/libtwo.tbd

cat <<EOF | $CC -mmacos-version-min=13.0 -o $t/a.o -c -xc -
void two(void);
int main() { two(); }
EOF
cat <<EOF | $CC -mmacos-version-min=13.0 -o $t/b.o -c -xc -
void one(void), two(void);
int main() { one(); two(); }
EOF

$CC --ld-path=$mold -mmacos-version-min=13.0 -o $t/exe1 $t/a.o -L$t -lone -ltwo
otool -L $t/exe1 > $t/log1
grep -Fq '/usr/lib/libold.dylib (compatibility version 1.0.0, current version 5.0.0)' $t/log1

$CC --ld-path=$mold -mmacos-version-min=13.0 -o $t/exe2 $t/b.o -L$t -lone -ltwo
otool -L $t/exe2 > $t/log2
grep -Fq '/usr/lib/libold.dylib (compatibility version 1.0.0, current version 3.0.0)' $t/log2
[ "$(grep -c libold $t/log2)" = 1 ]
