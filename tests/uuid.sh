#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

$CC --ld-path=$mold -o $t/exe1 $t/a.o
otool -l $t/exe1 | grep -A2 LC_UUID | grep -v '00000000-0000'

# -no_uuid drops LC_UUID, as ld-prime does - though dyld then refuses
# to load the image ("missing LC_UUID load command").
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_uuid
otool -l $t/exe2 > $t/lc2
not grep -q LC_UUID $t/lc2
$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -Wl,-no_uuid
otool -l $t/b.dylib > $t/lc3
not grep -q LC_UUID $t/lc3

# Identical inputs (and output path - the code signature embeds the
# basename) produce identical UUIDs
u1=$(otool -l $t/exe1 | grep uuid)
$CC --ld-path=$mold -o $t/exe1 $t/a.o
[ "$u1" = "$(otool -l $t/exe1 | grep uuid)" ]
