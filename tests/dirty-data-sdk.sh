#!/bin/bash
source "$(dirname "$0")"/common.inc

# An OS dylib bound for the shared cache with no -dirty_data_list gets
# the one an Apple-internal SDK has for it: the first -syslibroot's
# AppleInternal/DirtyDataFiles/<install name's leaf>.dirty, whose lines
# name symbols alone (a pattern matches nothing).
sdk=$(xcrun --show-sdk-path)
rm -rf $t/sdk
mkdir -p $t/sdk/AppleInternal/DirtyDataFiles
ln -s $sdk/usr $t/sdk/usr
ln -s $sdk/System $t/sdk/System
printf '_dd1\n_dd*\n' > $t/sdk/AppleInternal/DirtyDataFiles/libdd.dylib.dirty
printf '_dd3\n' > $t/list.txt

cat <<EOF | $CC -o $t/a.o -c -xc -
int dd1 = 1, dd2 = 2, dd3 = 3;
int ddf(void) { return dd1 + dd2 + dd3; }
EOF

link() {
  $CC --ld-path=$mold -shared -o $t/$1 $t/a.o -Wl,-syslibroot,$t/sdk -isysroot $t/sdk "${@:2}"
  nm -m $t/$1 | grep -E '_dd[0-9]' | sed 's/^[0-9a-f]* //' | tr '\n' ' ' > $t/$1.nm
}

link a.dylib -Wl,-install_name,/usr/lib/libdd.dylib
grep -q '(__DATA_DIRTY,__data) external _dd1 (__DATA,__data) external _dd2 (__DATA,__data) external _dd3' $t/a.dylib.nm

# Not for a dylib that stays out of the shared cache, nor when the
# command line gives a list.
link b.dylib -Wl,-install_name,/usr/lib/libdd.dylib -Wl,-not_for_dyld_shared_cache
not grep -q __DATA_DIRTY $t/b.dylib.nm
link c.dylib -Wl,-install_name,@rpath/libdd.dylib
not grep -q __DATA_DIRTY $t/c.dylib.nm
link d.dylib -Wl,-install_name,/usr/lib/libdd.dylib -Wl,-dirty_data_list,$t/list.txt
grep -q '(__DATA,__data) external _dd1 (__DATA,__data) external _dd2 (__DATA_DIRTY,__data) external _dd3' $t/d.dylib.nm
