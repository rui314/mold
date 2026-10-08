#!/bin/bash
source "$(dirname "$0")"/common.inc

# A private re-export's exports count as the re-exporting library's:
# libA re-exports libB from a non-public path, so a symbol of libB
# binds to libA. But when libB is also in the link itself - named on
# the command line or auto-linked, before or after libA - ld-prime
# binds the symbol to libB, the library that defines it. (Swift's
# libswiftDarwin privately re-exports libswift_Builtin_float this way.)
mkdir -p $t/sub
cat > $t/libA.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-$PLATFORM ]
install-name:    '$t/libA.dylib'
reexported-libraries:
  - targets:         [ $ARCH-$PLATFORM ]
    libraries:       [ '$t/sub/libB.dylib' ]
exports:
  - targets:         [ $ARCH-$PLATFORM ]
    symbols:         [ _a_only ]
...
EOF
cat > $t/sub/libB.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-$PLATFORM ]
install-name:    '$t/sub/libB.dylib'
exports:
  - targets:         [ $ARCH-$PLATFORM ]
    symbols:         [ _sym ]
...
EOF
cp $t/sub/libB.tbd $t/libB.tbd

echo 'extern int sym; int main() { return (long)&sym == 0; }' | $CC -o $t/a.o -c -xc -
printf '.linker_option "-lA"\n.linker_option "-lB"\n' | $CC -o $t/al.o -c -xassembler -

from() { nm -m $1 | grep ' _sym ' | sed 's/.*(from \(.*\))/\1/'; }

$CC --ld-path=$mold -o $t/exe1 $t/a.o -L$t -lA
[ "$(from $t/exe1)" = libA ]

$CC --ld-path=$mold -o $t/exe2 $t/a.o -L$t -lA -lB
[ "$(from $t/exe2)" = libB ]

$CC --ld-path=$mold -o $t/exe3 $t/a.o -L$t -lB -lA
[ "$(from $t/exe3)" = libB ]

$CC --ld-path=$mold -o $t/exe4 $t/a.o $t/al.o -L$t
[ "$(from $t/exe4)" = libB ]
otool -L $t/exe4 > $t/libs4
not grep -q libA.dylib $t/libs4
