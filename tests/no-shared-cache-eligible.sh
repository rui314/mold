#!/bin/bash
source "$(dirname "$0")"/common.inc

# -no_shared_cache_eligible keeps an image out of the dyld shared cache
# as -not_for_dyld_shared_cache does, and marks it so with an empty
# LC_SEGMENT_SPLIT_INFO, whatever the kind of image.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 3; }
int main() { return 0; }
EOF

split_size() {
  otool -l $1 > $t/cmds
  grep -A3 LC_SEGMENT_SPLIT_INFO $t/cmds | awk '/datasize/ { print $2 }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-no_shared_cache_eligible
[ "$(split_size $t/exe)" = 0 ]
$CC --ld-path=$mold -o $t/exe2 $t/a.o
[ "$(split_size $t/exe2)" = '' ]

# An OS dylib, which would be bound for the cache, has no split info
# left, and none of the cache's restrictions.
$CC --ld-path=$mold -o $t/libfoo.dylib -shared $t/a.o \
  -Wl,-install_name,/usr/lib/libfoo.dylib -Wl,-rpath,/x -Wl,-flat_namespace \
  -Wl,-no_shared_cache_eligible 2> $t/log
[ "$(split_size $t/libfoo.dylib)" = 0 ]
not grep -q warning $t/log

# Nor does -add_split_seg_info give any.
$CC --ld-path=$mold -o $t/libbar.dylib -shared $t/a.o \
  -Wl,-add_split_seg_info -Wl,-no_shared_cache_eligible
[ "$(split_size $t/libbar.dylib)" = 0 ]
