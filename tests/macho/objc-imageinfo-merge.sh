#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime merges the objects' __objc_imageinfo flags as it checks each
# object: the image's categories may have class properties only if
# every object's may, and an object differing from those before it in
# that draws a warning; the Swift language version is the oldest one.
mk() {
  {
    echo '.section __DATA,__objc_imageinfo,regular,no_dead_strip'
    echo '.long 0'
    echo ".long $2"
    [ -z "$3" ] || printf '.globl _main\n.text\n_main: ret\n'
  } | $CC -o $t/$1.o -c -xassembler -
}
mk a 0x40 main
mk b 0
mk c 0x40
mk d 0
dir=$t

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o $t/d.o 2> $t/log
grep -F "warning: mixed ObjC ABI, $dir/b.o compiled without category class properties" $t/log
grep -F "warning: mixed ObjC ABI, $dir/c.o compiled with category class properties" $t/log
not grep -F "$dir/d.o" $t/log
otool -s __DATA_CONST __objc_imageinfo $t/exe > $t/info
grep -Eq '00000000 00000000|00 00 00 00 00 00 00 00' $t/info

$CC --ld-path=$mold -o $t/exe $t/a.o $t/c.o 2> $t/log
not grep -F 'mixed ObjC ABI' $t/log

mk e 0x05000740 main
mk f 0x06010740
$CC --ld-path=$mold -o $t/exe $t/f.o $t/e.o
otool -s __DATA_CONST __objc_imageinfo $t/exe > $t/info
grep -Eq '00000000 05000740|00 00 00 00 40 07 00 05' $t/info
