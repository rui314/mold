#!/bin/bash
source "$(dirname "$0")"/common.inc

# An image's __objc_imageinfo keeps only the flags that describe its
# code - the Swift version bytes, category class properties (0x40) and
# signed class_ro_t pointers (0x10) - even when a single object has
# one: the simulator bit (0x20) clang sets, the old garbage collector's
# and dyld's own go, in a -r output too.
mk() {
  {
    echo '.section __DATA,__objc_imageinfo,regular,no_dead_strip'
    echo '.long 0'
    echo ".long $2"
    [ -z "$3" ] || printf '.globl _main\n.text\n_main: ret\n'
  } | $CC -o $t/$1.o -c -xassembler -
}

# Whether an image's (or object's) flags are the 8 hex digits given.
flags_are() {
  local seg=$(otool -l $1 | grep -A1 'sectname __objc_imageinfo' | awk '$1 == "segname" { print $2 }')
  local f=$2
  otool -s $seg __objc_imageinfo $1 |
    grep -Eq "00000000 $f|00 00 00 00 ${f:6:2} ${f:4:2} ${f:2:2} ${f:0:2}"
}

for pair in 0x60:00000040 0xff:00000050 0x0702ff:00070250 0x62:00000040 0x05000740:05000740; do
  mk a ${pair%:*} main
  $CC --ld-path=$mold -o $t/exe $t/a.o
  flags_are $t/exe ${pair#*:}
  $mold -r -o $t/r.o $t/a.o
  flags_are $t/r.o ${pair#*:}
done

# A simulator's objects all have the simulator bit.
if on_simulator; then
  simcc=$CC
elif sdk=$(xcrun --sdk iphonesimulator --show-sdk-path 2> /dev/null); then
  simcc="cc -target $ARCH-apple-ios17.0-simulator -isysroot $sdk"
fi
if [ -n "$simcc" ]; then
  cat <<EOF | $simcc -o $t/sim.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject
@end
@implementation Foo
@end
int main(void) { return 0; }
EOF
  flags_are $t/sim.o 00000060
  $simcc --ld-path=$mold -o $t/sim $t/sim.o -framework Foundation
  flags_are $t/sim 00000040
fi
