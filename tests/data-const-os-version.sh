#!/bin/bash
source "$(dirname "$0")"/common.inc

# dyld makes __DATA_CONST read-only once it has applied an image's
# fixups. ld-prime gives an executable, a dylib or a bundle that
# segment by default only from macOS 10.15 (ld64's version2019Fall),
# but not for 10.15.4 up to 10.16, and not with -no_pie (which arm64
# ignores for the executable, and which means nothing to a dylib),
# unless the image is bound for the dyld shared cache. -data_const and
# -no_data_const decide either way.
cat <<EOF | $CC -o $t/a.o -c -xc -
int x = 42;
int *const p = &x;
int main() { return *p - 42; }
EOF

sdk=$(xcrun --show-sdk-path)
segs() {
  $mold -arch $ARCH $t/a.o -platform_version macos "$@" -syslibroot $sdk -lSystem -o $t/out \
    2> /dev/null
  otool -l $t/out | awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }'
}

for kind in '' -dylib -bundle; do
  for version in 10.14.6 10.15.4 10.15.7; do
    segs $version 15.0 $kind > $t/segs1
    not grep -q __DATA_CONST $t/segs1
  done
  for version in 10.15 10.15.3 10.16 13.0; do
    segs $version 15.0 $kind > $t/segs2
    grep -q __DATA_CONST $t/segs2
  done
  segs 13.0 15.0 $kind -no_pie > $t/segs3
  not grep -q __DATA_CONST $t/segs3
done

segs 10.14 15.0 -data_const > $t/segs4
grep -q __DATA_CONST $t/segs4
segs 10.15 15.0 -no_data_const > $t/segs5
not grep -q __DATA_CONST $t/segs5
segs 10.14 15.0 -dylib -install_name /usr/lib/libfoo.dylib > $t/segs6
grep -q __DATA_CONST $t/segs6
segs 13.0 15.0 -dylib -install_name /usr/lib/libfoo.dylib -no_pie > $t/segs7
grep -q __DATA_CONST $t/segs7

# Firmware has the segment unless it is a non-PIE executable.
$mold -arch $ARCH -dylib $t/a.o -platform_version firmware 1.0 1.0 -no_pie -o $t/fw.dylib
otool -l $t/fw.dylib | grep -q 'segname __DATA_CONST'

# A non-PIE executable has none even when bound for the shared region,
# but arm64 makes every executable PIE.
segs 13.0 15.0 -no_pie -add_split_seg_info > $t/segs8
if [ $ARCH = arm64 ]; then
  grep -q __DATA_CONST $t/segs8
else
  not grep -q __DATA_CONST $t/segs8
fi
