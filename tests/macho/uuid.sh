#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

$CC --ld-path=$mold -o $t/exe1 $t/a.o
otool -l $t/exe1 | grep -A2 LC_UUID | grep -qv '00000000-0000'

# dyld requires LC_UUID, so -no_uuid zeroes it instead of dropping it
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-no_uuid
otool -l $t/exe2 | grep -A2 LC_UUID | grep -q '00000000-0000-0000-0000-000000000000'
$t/exe2

# Identical inputs (and output path - the code signature embeds the
# basename) produce identical UUIDs
u1=$(otool -l $t/exe1 | grep uuid)
$CC --ld-path=$mold -o $t/exe1 $t/a.o
[ "$u1" = "$(otool -l $t/exe1 | grep uuid)" ]
