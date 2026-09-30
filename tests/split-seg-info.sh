#!/bin/bash
source "$(dirname "$0")"/common.inc

# -add_split_seg_info records where the image refers from one section
# to another (LC_SEGMENT_SPLIT_INFO), so that a dyld shared cache or
# kernel collection builder can slide its segments apart. ld64 and
# ld-prime spell it only so: there is no negative form, and
# -split_seg_info is no option of theirs.
cat <<EOF | $CC -o $t/a.o -c -xc -O1 -
int x = 3;
int *p = &x;
int f(void) { return *p + x; }
EOF

$mold -arch $ARCH -dylib -lSystem -syslibroot "$(xcrun --show-sdk-path)" \
  $t/a.o -o $t/b.dylib
otool -l $t/b.dylib > $t/lc
not grep -q 'cmd LC_SEGMENT_SPLIT_INFO$' $t/lc

$mold -arch $ARCH -dylib -lSystem -syslibroot "$(xcrun --show-sdk-path)" \
  -add_split_seg_info $t/a.o -o $t/c.dylib
otool -l $t/c.dylib | grep -q 'cmd LC_SEGMENT_SPLIT_INFO$'

for opt in -split_seg_info -no_split_seg_info -no_add_split_seg_info; do
  not $mold -arch $ARCH -dylib -lSystem -syslibroot "$(xcrun --show-sdk-path)" \
    $opt $t/a.o -o $t/d.dylib 2> $t/log
  grep -q "unknown .*option.*$opt" $t/log
done
