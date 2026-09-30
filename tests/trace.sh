#!/bin/bash
source "$(dirname "$0")"/common.inc

# -t lists every file the link loads, on stdout: the objects, every
# member of an archive whether used or not, and each library a stub
# re-exports - libSystem's own dozens included - by the file it was
# found at, or by its install name if a stub inlines it and no file
# holds it (ld-prime, which prints them in varying order).
cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void unused_member() {}
EOF

rm -f $t/lib.a
ar rcs $t/lib.a $t/a.o $t/b.o

cat <<EOF | $CC -o $t/main.o -c -xc -
void foo();
int b(void);
int main() { foo(); return b(); }
EOF

cat > $t/libA.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ arm64-macos, x86_64-macos ]
install-name:    '/usr/local/lib/libA.dylib'
reexported-libraries:
  - targets:     [ arm64-macos, x86_64-macos ]
    libraries:   [ '/usr/local/lib/libB.dylib' ]
exports:
  - targets:     [ arm64-macos, x86_64-macos ]
    symbols:     [ _a ]
--- !tapi-tbd
tbd-version:     4
targets:         [ arm64-macos, x86_64-macos ]
install-name:    '/usr/local/lib/libB.dylib'
exports:
  - targets:     [ arm64-macos, x86_64-macos ]
    symbols:     [ _b ]
...
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a $t/libA.tbd -Wl,-t > $t/log
grep -q '/main.o$' $t/log
grep -q 'lib.a(a.o)$' $t/log
grep -q 'lib.a(b.o)$' $t/log
grep -q '/usr/lib/libSystem.tbd$' $t/log
grep -q '/usr/lib/system/libsystem_c.tbd$' $t/log
grep -q '/libA.tbd$' $t/log
grep -q '^/usr/local/lib/libB.dylib$' $t/log
[ "$(grep -c 'lib.a(a.o)$' $t/log)" = 1 ]
