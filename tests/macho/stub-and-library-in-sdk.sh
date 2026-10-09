#!/bin/bash
source "$(dirname "$0")"/common.inc

# A library search that finds both a stub and the library it stands for
# takes the stub - but in an SDK (a path with .sdk/ in it), where a
# library has no business next to its stub, ld-prime warns and takes the
# library, unless $LD_PREFER_TAPI_FILE is set.
mkdir -p $t/Fake.sdk/usr/lib $t/plain
echo 'int foo(void) { return 1; }' | $CC -dynamiclib -o $t/Fake.sdk/usr/lib/libq.dylib \
  -xc - -install_name /usr/lib/libq_dylib.dylib
cp $t/Fake.sdk/usr/lib/libq.dylib $t/plain/libq.dylib
cat > $t/Fake.sdk/usr/lib/libq.tbd <<EOF
--- !tapi-tbd
tbd-version: 4
targets: [ $ARCH-$PLATFORM ]
install-name: /usr/lib/libq_tbd.dylib
exports:
  - targets: [ $ARCH-$PLATFORM ]
    symbols: [ _foo ]
...
EOF
cp $t/Fake.sdk/usr/lib/libq.tbd $t/plain/libq.tbd
echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/a.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t/Fake.sdk/usr/lib -lq 2> $t/log
grep -q "warning: text-based stub file $t/Fake.sdk/usr/lib/libq.tbd and library file $t/Fake.sdk/usr/lib/libq.dylib unexpectedly found. Falling back to library file for linking." $t/log
otool -L $t/exe | grep -q /usr/lib/libq_dylib.dylib

LD_PREFER_TAPI_FILE=1 $CC --ld-path=$mold -o $t/exe $t/a.o -L$t/Fake.sdk/usr/lib -lq 2> $t/log
not grep -q warning $t/log
otool -L $t/exe | grep -q /usr/lib/libq_tbd.dylib

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t/plain -lq 2> $t/log
not grep -q warning $t/log
otool -L $t/exe | grep -q /usr/lib/libq_tbd.dylib
