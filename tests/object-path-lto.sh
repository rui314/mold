#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -flto
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -flto -Wl,-object_path_lto,$t/obj
$t/exe | grep 'Hello world'
otool -l $t/obj > /dev/null

# A directory gets the object as lto.o in it: ld-prime appends "/lto.o"
# to the path as given, which names the object everywhere.
rm -rf $t/dir
mkdir $t/dir
$CC --ld-path=$mold -o $t/exe2 $t/a.o -flto -Wl,-object_path_lto,$t/dir/ -Wl,-map,$t/map
$t/exe2 | grep 'Hello world'
otool -l $t/dir/lto.o > /dev/null
grep -q "^\[ *[0-9]*\] $t/dir//lto.o\$" $t/map

# A path the object can't be written to is no error.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -flto -Wl,-object_path_lto,$t/nodir/obj 2> $t/log
$t/exe3 | grep 'Hello world'
not grep -q . $t/log
