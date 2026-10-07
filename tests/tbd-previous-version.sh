#!/bin/bash
source "$(dirname "$0")"/common.inc

# The $ld$ directives name macOS and its versions.
on_simulator && skip

# An $ld$previous directive for the whole library gives it the older
# library's install name and, if it says, version. A library of the
# link whose install name that is by its own right decides the load
# command.
cat > $t/libfoo.tbd <<'EOF'
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '/usr/lib/libfoo.dylib'
current-version: 3
compatibility-version: 1.1
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _foo, '$ld$previous$/usr/lib/libbar.dylib$1.5$1$10.6$16.0$$' ]
...
EOF

sed 's/\$1\.5\$1/$$1/' $t/libfoo.tbd > $t/libfoo2.tbd

cat > $t/libbar.tbd <<'EOF'
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '/usr/lib/libbar.dylib'
current-version: 7
compatibility-version: 2
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _bar ]
...
EOF

cat <<'EOF' | $CC -mmacos-version-min=13.0 -o $t/a.o -c -xc -
void foo(void);
int main() { foo(); }
EOF

$CC --ld-path=$mold -mmacos-version-min=13.0 -o $t/exe1 $t/a.o $t/libfoo.tbd
otool -L $t/exe1 | grep -Fq '/usr/lib/libbar.dylib (compatibility version 1.5.0, current version 1.5.0)'

$CC --ld-path=$mold -mmacos-version-min=13.0 -o $t/exe2 $t/a.o $t/libfoo2.tbd
otool -L $t/exe2 | grep -Fq '/usr/lib/libbar.dylib (compatibility version 1.1.0, current version 3.0.0)'

$CC --ld-path=$mold -mmacos-version-min=13.0 -o $t/exe3 $t/a.o $t/libfoo.tbd $t/libbar.tbd
otool -L $t/exe3 > $t/log
grep -Fq '/usr/lib/libbar.dylib (compatibility version 2.0.0, current version 7.0.0)' $t/log
not grep -q libfoo $t/log
