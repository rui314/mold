#!/bin/bash
source "$(dirname "$0")"/common.inc

# -t lists every file the link reads, once each, on stdout: the objects,
# every member of an archive whether used or not, and each library a
# stub re-exports - libSystem's own dozens included - by the file it was
# found at, or by its install name if a stub inlines it and no file
# holds it.
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
targets:         [ arm64-$PLATFORM, x86_64-$PLATFORM ]
install-name:    '/usr/local/lib/libA.dylib'
reexported-libraries:
  - targets:     [ arm64-$PLATFORM, x86_64-$PLATFORM ]
    libraries:   [ '/usr/local/lib/libB.dylib' ]
exports:
  - targets:     [ arm64-$PLATFORM, x86_64-$PLATFORM ]
    symbols:     [ _a ]
--- !tapi-tbd
tbd-version:     4
targets:         [ arm64-$PLATFORM, x86_64-$PLATFORM ]
install-name:    '/usr/local/lib/libB.dylib'
exports:
  - targets:     [ arm64-$PLATFORM, x86_64-$PLATFORM ]
    symbols:     [ _b ]
...
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a $t/libA.tbd -Wl,-t > $t/log
grep -q '/main.o$' $t/log
grep -q 'lib.a(a.o)$' $t/log
grep -q 'lib.a(b.o)$' $t/log
grep -q '/usr/lib/libSystem.tbd$' $t/log
if on_simulator; then
  # A simulator's libSystem stub inlines the libraries it re-exports.
  grep -q '^/usr/lib/system/libsystem_c.dylib$' $t/log
else
  grep -q '/usr/lib/system/libsystem_c.tbd$' $t/log
fi
grep -q '/libA.tbd$' $t/log
grep -q '^/usr/local/lib/libB.dylib$' $t/log
[ "$(grep -c 'lib.a(a.o)$' $t/log)" = 1 ]

# A library loaded as another's re-export (Foundation's libobjc.A.tbd)
# that a naming finds at another path (-lobjc's libobjc.tbd, a symlink
# to it) is listed by both files, each read. (ld-prime lists the
# naming's file alone.)
echo 'int main() { return 0; }' | $CC -o $t/c.o -c -xc -
$CC --ld-path=$mold -o $t/exe2 $t/c.o -framework Foundation -Wl,-t > $t/log2
grep -q '/usr/lib/libobjc.A.tbd$' $t/log2
$CC --ld-path=$mold -o $t/exe3 $t/c.o -framework Foundation -lobjc -Wl,-t > $t/log3
grep -q '/usr/lib/libobjc.tbd$' $t/log3
[ "$(sort $t/log3 | uniq -d)" = "" ]
