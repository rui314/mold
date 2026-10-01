#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib records the dylibs it loads, by install name and
# versions, in the order of its load commands, and its imports by the
# library of each; an image that merges it, linked by either linker,
# loads them in its place.
mkdir -p $t/sub
cat <<EOF | $CC -o $t/w.o -c -xc -
int w(void) { return 7; }
EOF
$CC -shared -o $t/sub/libw.dylib $t/w.o -Wl,-install_name,@rpath/libw.dylib \
  -Wl,-current_version,3.2.1 -Wl,-compatibility_version,3.0

cat <<EOF | $CC -o $t/a.o -c -xc -
int w(void);
int a(void) { return w() + 1; }
EOF
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -L$t/sub -lw -lz \
  -Wl,-make_mergeable -Wl,-install_name,@rpath/libfoo.dylib

cat <<EOF | $CC -o $t/main.o -c -xc -
int a(void);
int main() { return a() == 8 ? 0 : 1; }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo -Wl,-rpath,@loader_path/sub
$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo -Wl,-rpath,@loader_path/sub
for exe in $t/exe $t/exe2; do
  $exe
  otool -L $exe > $t/libs
  grep -A2 libSystem $t/libs | grep -q '@rpath/libw.dylib (compatibility version 3.0.0, current version 3.2.1)'
  grep -A2 libSystem $t/libs | grep -q 'libz'
  not grep -q libfoo $t/libs
  dyld_info -fixups $exe | grep -q 'libw/_w'
done
