#!/bin/bash
source "$(dirname "$0")"/common.inc

# A library can name the clients allowed to link it directly
# (allowable-clients in a .tbd, as SwiftUICore names AppKit, SwiftUI,
# UIKit and a few more). ld-prime refuses any other output: an error
# for a library on the command line, while an auto-linked one is left
# out with a warning if it would have provided a symbol. The client
# name - -client_name, else the output's leaf less "lib", cut at the
# first '.' or '_' - may be any prefix of an allowed one. Such a library
# is never bound to through a re-export, even from a public location:
# its symbols bind to the library that re-exports it.
mkdir -p $t/root/usr/lib
cat > $t/root/usr/lib/libpub.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-$PLATFORM ]
install-name:    '/usr/lib/libpub.dylib'
allowable-clients:
  - targets:         [ $ARCH-$PLATFORM ]
    clients:         [ umb, Friend ]
exports:
  - targets:         [ $ARCH-$PLATFORM ]
    symbols:         [ _sym ]
...
EOF
cat > $t/root/usr/lib/libumb.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-$PLATFORM ]
install-name:    '/usr/lib/libumb.dylib'
reexported-libraries:
  - targets:         [ $ARCH-$PLATFORM ]
    libraries:       [ '/usr/lib/libpub.dylib' ]
exports:
  - targets:         [ $ARCH-$PLATFORM ]
    symbols:         [ _umb ]
...
EOF
cat > $t/libjson.tbd <<EOF
{
  "tapi_tbd_version": 5,
  "main_library": {
    "target_info": [ { "target": "$ARCH-macos" } ],
    "install_names": [ { "name": "/usr/lib/libjson.dylib" } ],
    "allowable_clients": [ { "clients": [ "Friend" ] } ],
    "exported_symbols": [ { "text": { "global": [ "_jsym" ] } } ]
  }
}
EOF

echo 'extern int sym; int main() { return (long)&sym == 0; }' | $CC -o $t/a.o -c -xc -
echo 'int main() { return 0; }' | $CC -o $t/m.o -c -xc -
R="-Wl,-syslibroot,$t/root -L$t/root/usr/lib"

$CC --ld-path=$mold -o $t/exe1 $t/a.o $R -lumb
nm -m $t/exe1 | grep -q '_sym (from libumb)'
otool -L $t/exe1 > $t/libs1
not grep -q libpub $t/libs1

not $CC --ld-path=$mold -o $t/exe2 $t/m.o $R -lpub 2> $t/log2
grep -q "cannot link directly with 'libpub.dylib' because product being built is not an allowed client of it" $t/log2
not $CC --ld-path=$mold -o $t/exe3 $t/m.o $t/libjson.tbd 2> $t/log3
grep -q "cannot link directly with 'libjson.dylib'" $t/log3

mkdir -p $t/out
$CC --ld-path=$mold -o $t/out/Friend $t/m.o $R -lpub
$CC --ld-path=$mold -o $t/out/Fri $t/m.o $R -lpub
$CC --ld-path=$mold -o $t/out/libFriend_x.dylib -shared $t/m.o $R -lpub
not $CC --ld-path=$mold -o $t/out/Friendly $t/m.o $R -lpub 2> /dev/null
$CC --ld-path=$mold -o $t/exe4 $t/m.o $R -lpub -Wl,-client_name,umb

echo '.linker_option "-lpub"' | $CC -o $t/al.o -c -xassembler -
$CC --ld-path=$mold -o $t/exe5 $t/m.o $t/al.o $R 2> $t/log5
not grep -q 'allowed client' $t/log5
not $CC --ld-path=$mold -o $t/exe6 $t/a.o $t/al.o $R 2> $t/log6
grep -q "Could not parse or use implicit file '.*libpub.tbd': cannot link directly with 'libpub.dylib'" $t/log6
