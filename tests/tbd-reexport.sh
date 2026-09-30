#!/bin/bash
source "$(dirname "$0")"/common.inc

mkdir -p $t/libs/SomeFramework.framework/

cat > $t/libs/SomeFramework.framework/SomeFramework.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
uuids:
  - target:          x86_64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/usr/frameworks/SomeFramework.framework/SomeFramework'
current-version: 0000
compatibility-version: 150
reexported-libraries:
  - targets:         [ x86_64-macos, arm64-macos ]
    libraries:       [ '/usr/lib/libbar.dylib' ]
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _foo ]
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
uuids:
  - target:          x86_64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
install-name:    '/usr/lib/libbar.dylib'
current-version: 0000
compatibility-version: 150
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _bar ]
...
EOF

cat <<EOF | $CC -o $t/a.o -c -xc -
extern void foo();
extern void bar() __attribute__((weak_import));

int main() {
  foo();
  if (bar)
    bar();
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -F$t/libs -Wl,-framework,SomeFramework

otool -L $t/exe | grep '/usr/frameworks/SomeFramework.framework/SomeFramework'

# The re-exported /usr/lib/libbar.dylib, inlined in the stub, lives in
# a public location, so ld-prime binds _bar to it directly and gives it
# a load command of its own - weak, since every reference to it is a
# weak import - after the libraries named on the command line.
otool -L $t/exe > $t/deps
grep -q '/usr/lib/libbar.dylib.*weak' $t/deps
dyld_info -fixups $t/exe | grep -q 'libbar/_bar \[weak-import\]'
dyld_info -fixups $t/exe | grep -q 'SomeFramework/_foo'
nm -m $t/exe | grep -q 'weak external _bar (from libbar)'

# A strong reference loads it strongly.
cat <<EOF | $CC -o $t/b.o -c -xc -
extern void foo();
extern void bar();
int main() { foo(); bar(); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/b.o -F$t/libs -Wl,-framework,SomeFramework
otool -L $t/exe2 > $t/deps2
grep '/usr/lib/libbar.dylib' $t/deps2 | not grep -q weak
dyld_info -fixups $t/exe2 | grep -q 'libbar/_bar$'

# A public library with a stub of its own loads from that stub, not
# from the copy inlined in the umbrella's.
mkdir -p $t/root/usr/lib
cat > $t/root/usr/lib/libbar.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '/usr/lib/libbar.dylib'
current-version: 5
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ _bar, _baz ]
...
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
extern void foo();
extern void bar();
extern void baz();
int main() { foo(); bar(); baz(); }
EOF
$CC --ld-path=$mold -o $t/exe4 $t/c.o -F$t/libs -Wl,-framework,SomeFramework \
  -Wl,-syslibroot,$t/root
otool -L $t/exe4 | grep -q '/usr/lib/libbar.dylib (.*current version 5.0.0)'
dyld_info -fixups $t/exe4 | grep -q 'libbar/_baz$'

# A re-exported library in a private location binds through the
# umbrella and gets no load command.
mkdir -p $t/priv/Priv.framework
sed 's|/usr/lib/libbar.dylib|/usr/lib/foo/libbar.dylib|; s|SomeFramework|Priv|g' \
  $t/libs/SomeFramework.framework/SomeFramework.tbd > $t/priv/Priv.framework/Priv.tbd
$CC --ld-path=$mold -o $t/exe3 $t/b.o -F$t/priv -Wl,-framework,Priv
otool -L $t/exe3 > $t/deps3
not grep -q libbar $t/deps3
dyld_info -fixups $t/exe3 | grep -q 'Priv/_bar'
