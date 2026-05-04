#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
#include <stdio.h>
int main() {
  printf("Hello world\n");
}
EOF

# Probe whether the host toolchain can link a PIE; skip if not.
$CC -o $t/probe $t/a.o -pie 2> /dev/null || skip

# Default: standard DT_RELR tags (35/36/37) and SHT_RELR (19).
$CC -B. -o $t/exe1 $t/a.o -pie -Wl,--pack-dyn-relocs=relr
readelf -SW $t/exe1 | grep -F .relr.dyn | grep -Ew 'RELR'
readelf --dynamic $t/exe1 > $t/log1
grep -Ew 'RELR|<unknown>: 24' $t/log1
grep -Ew 'RELRSZ|<unknown>: 23' $t/log1
grep -Ew 'RELRENT|<unknown>: 25' $t/log1

# --use-android-relr-tags: legacy DT_ANDROID_RELR* (in the OS-specific
# range) and SHT_ANDROID_RELR section type, for compatibility with
# pre-Android-12 loaders that don't recognize the standard tags.
$CC -B. -o $t/exe2 $t/a.o -pie \
  -Wl,--pack-dyn-relocs=relr -Wl,--use-android-relr-tags
readelf -SW $t/exe2 | grep -F .relr.dyn | grep -Ev '\bRELR\b'
readelf --dynamic $t/exe2 > $t/log2
grep -E '0x0*6fffe000\b' $t/log2
grep -E '0x0*6fffe001\b' $t/log2
grep -E '0x0*6fffe003\b' $t/log2
# Make sure the standard tags are NOT emitted in this mode.
not grep -Ew 'RELR|<unknown>: 24' $t/log2
not grep -Ew 'RELRSZ|<unknown>: 23' $t/log2
not grep -Ew 'RELRENT|<unknown>: 25' $t/log2

# --no-use-android-relr-tags reverts to the standard tags.
$CC -B. -o $t/exe3 $t/a.o -pie \
  -Wl,--pack-dyn-relocs=relr \
  -Wl,--use-android-relr-tags -Wl,--no-use-android-relr-tags
readelf --dynamic $t/exe3 > $t/log3
grep -Ew 'RELR|<unknown>: 24' $t/log3
