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

# -random_uuid takes a random version-4 UUID instead, and the last of it
# and -no_uuid counts.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-random_uuid
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-random_uuid
u3=$(otool -l $t/exe3 | grep uuid)
u4=$(otool -l $t/exe4 | grep uuid)
[ "$u3" != "$u4" ]
echo "$u3" | grep -Eq 'uuid [0-9A-F]{8}-[0-9A-F]{4}-4[0-9A-F]{3}-[89AB][0-9A-F]{3}-[0-9A-F]{12}$'
if [ $ARCH = arm64 ]; then
  codesign -v $t/exe3
fi
$CC --ld-path=$mold -o $t/exe5 $t/a.o -Wl,-random_uuid,-no_uuid
otool -l $t/exe5 > $t/lc5
not grep -q LC_UUID $t/lc5
$CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-no_uuid,-random_uuid
otool -l $t/exe6 | grep -A2 LC_UUID | grep -v '00000000-0000'
