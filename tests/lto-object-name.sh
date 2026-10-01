#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -flto -g -mmacosx-version-min=27.0 -c -xc - -o $t/a.o
#include <stdio.h>
int main() { printf("Hello\n"); }
EOF

# ld-prime calls the object LTO compiles /tmp/lto.o, though it writes
# no such file: diagnostics and the map name it so, and so do the debug
# notes, with modification time 0. It is no input to depend on.
$CC --ld-path=$mold -flto -mmacosx-version-min=26.0 -o $t/exe $t/a.o \
  -Wl,-map,$t/map -Wl,-dependency_info,$t/dep 2> $t/log
grep -q ': warning: object file (/tmp/lto.o) was built for newer' $t/log
grep -q '^\[ *[0-9]*\] /tmp/lto.o$' $t/map
nm -ap $t/exe > $t/nm
grep -q '^0000000000000000 - .. 0001   OSO /tmp/lto.o$' $t/nm
not grep -q -a -F /tmp/lto.o $t/dep
$t/exe | grep -q Hello

# With -object_path_lto, the object is named after the file written.
$CC --ld-path=$mold -flto -o $t/exe2 $t/a.o -Wl,-object_path_lto,$t/lto.o \
  -Wl,-map,$t/map2 2> /dev/null
grep -q "^\[ *[0-9]*\] $t/lto.o\$" $t/map2
nm -ap $t/exe2 > $t/nm2
grep -q " OSO $(pwd)/$t/lto.o\$" $t/nm2
not grep -q '^0000000000000000 .* OSO ' $t/nm2
