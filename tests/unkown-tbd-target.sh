#!/bin/bash
source "$(dirname "$0")"/common.inc

# TAPI refuses a .tbd naming a target of a platform it doesn't know,
# pointing at the target in the file.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { foo(); }
EOF

cat > $t/b.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-bar, arm64-bar ]
uuids:
  - target:          x86_64-bar
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-bar
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/usr/lib/bar'
current-version: 0
compatibility-version: 0
exports:
  - targets:         [ x86_64-bar ]
    symbols:         [ _foo ]
  - targets:         [ arm64-bar ]
    symbols:         [ _foo ]
...
EOF

not $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.tbd 2> $t/log
grep -q 'tapi error: malformed file$' $t/log
grep -A3 "^/.*/$t/b.tbd:3:20: error: unknown target$" $t/log > $t/diag
diff - $t/diag <<EOF
$(cd $t && pwd -P)/b.tbd:3:20: error: unknown target
targets:         [ x86_64-bar, arm64-bar ]
                   ^~~~~~~~~~
 in '$t/b.tbd'
EOF
