#!/bin/bash
source "$(dirname "$0")"/common.inc

# A version 1-3 .tbd names its platform in a key of its own, spelled as
# TAPI's YAML reader knows it (macosx, ios, iosmac, zippered ...; not
# macos). ld-prime refuses a file with another or none, pointing at it.
tbd() {
  cat > $t/$1.tbd <<EOF
--- !tapi-tbd-v3
archs:           [ $ARCH ]
$2
install-name:    '/usr/lib/lib$1.dylib'
exports:
  - archs:           [ $ARCH ]
    symbols:         [ _foo ]
...
EOF
}
tbd good 'platform:        macosx'
tbd bad 'platform:        macos'
tbd none ''
dir=$(cd $t && pwd -P)

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { return foo(); }
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o $t/good.tbd
otool -L $t/exe | grep -q /usr/lib/libgood.dylib

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/bad.tbd 2> $t/log
grep -q 'tapi error: malformed file$' $t/log
grep -A3 "^$dir/bad.tbd:3:18: error: unknown platform$" $t/log > $t/diag
diff - $t/diag <<EOF
$dir/bad.tbd:3:18: error: unknown platform
platform:        macos
                 ^~~~~
 in '$t/bad.tbd'
EOF

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/none.tbd 2> $t/log
grep -A3 "^$dir/none.tbd:2:1: error: missing required key 'platform'$" $t/log > $t/diag
diff - $t/diag <<EOF
$dir/none.tbd:2:1: error: missing required key 'platform'
archs:           [ $ARCH ]
^
 in '$t/none.tbd'
EOF
