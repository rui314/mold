#!/bin/bash
source "$(dirname "$0")"/common.inc

# TAPI reads a .tbd by the YAML schema of the version its tag names (none
# or !tapi-tbd-v1, -v2, -v3, or !tapi-tbd for version 4), and ld-prime
# refuses a file with a key that schema doesn't have - objc-eh-types
# came with version 3, weak-def-symbols became weak-symbols in version 4
# - or lacks one it requires, gives one twice, or has a sequence where
# the schema wants a mapping or the like. It names the first fault: a
# key given twice as it reads the file; then by the schema's order, a
# nested mapping wholly first; and the keys unknown last, by name.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { return foo(); }
EOF
dir=$(cd $t && pwd -P)

# check <name> <line:col> <message> <caret line>, with the file on stdin
check() {
  cat > $t/$1.tbd
  not $CC --ld-path=$mold -o $t/exe $t/a.o $t/$1.tbd 2> $t/log
  grep -q 'tapi error: malformed file$' $t/log
  grep -A3 "^$dir/$1.tbd:$2: error: " $t/log > $t/diag
  line=$(sed -n "$(echo $2 | cut -d: -f1)p" $t/$1.tbd)
  printf '%s\n%s\n%s\n %s\n' "$dir/$1.tbd:$2: error: $3" "$line" "$4" "in '$t/$1.tbd'" |
    diff - $t/diag
}

check v2-eh 8:5 "unknown key 'objc-eh-types'" '    ^~~~~~~~~~~~~' <<EOF
--- !tapi-tbd-v2
archs:           [ $ARCH ]
platform:        macosx
install-name:    /usr/lib/libv2.dylib
exports:
  - archs:           [ $ARCH ]
    symbols:         [ _foo ]
    objc-eh-types:   [ Foo ]
...
EOF

check v3-swift 5:1 "unknown key 'swift-version'" '^~~~~~~~~~~~~' <<EOF
--- !tapi-tbd-v3
archs:           [ $ARCH ]
platform:        macosx
install-name:    /usr/lib/libv3.dylib
swift-version:   5
exports:
  - archs:           [ $ARCH ]
    symbols:         [ _foo ]
...
EOF

check v4-weak 8:5 "unknown key 'weak-def-symbols'" '    ^~~~~~~~~~~~~~~~' <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    /usr/lib/libv4.dylib
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _foo ]
    weak-def-symbols: [ _bar ]
...
EOF

# Of two unknown keys the first by name, after the nested mappings.
check order 8:1 "unknown key 'aaa'" '^~~' <<EOF
--- !tapi-tbd-v3
archs:           [ $ARCH ]
zzz:             1
platform:        macosx
install-name:    /usr/lib/liborder.dylib
exports:
  - archs:           [ $ARCH ]
aaa:             1
...
EOF

check dup 8:5 "duplicated mapping key 'symbols'" '    ^~~~~~~' <<EOF
--- !tapi-tbd-v3
archs:           [ $ARCH ]
platform:        macosx
install-name:    /usr/lib/libdup.dylib
exports:
  - archs:           [ $ARCH ]
    symbols:         [ _foo ]
    symbols:         [ _bar ]
bogus:           1
...
EOF

check missing 6:5 "missing required key 'clients'" '    ^' <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    /usr/lib/libmissing.dylib
allowable-clients:
  - targets:         [ $ARCH-macos ]
    libraries:       [ Foo ]
...
EOF

check tag 2:1 'unsupported file type' '^' <<EOF
--- !tapi-tbd-v9
archs:           [ $ARCH ]
platform:        macosx
install-name:    /usr/lib/libtag.dylib
...
EOF

# A version 3 file's keys under a version 4 tag.
check scalar 5:18 'not a sequence' '                 ^~~' <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    /usr/lib/libscalar.dylib
parent-umbrella: Foo
...
EOF

check flow 5:22 'not a mapping' '                     ^~~~' <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    /usr/lib/libflow.dylib
allowable-clients: [ Foo ]
...
EOF
