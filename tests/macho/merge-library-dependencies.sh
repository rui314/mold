#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib records the dylibs it links - install name and
# versions - and an image that merges it gets a load command for each,
# after those of its own command line, without reading the library
# (ld-prime doesn't look for it) and whether or not anything binds to
# it. The merged code's imports bind to it. A library the command line
# names takes the record's place: -weak-l makes it, and the imports
# from it, weak.
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
$CC -shared -o $t/libfoo.dylib $t/a.o -L$t/sub -lw -lz -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib

cat <<EOF | $CC -o $t/main.o -c -xc -
int a(void);
int main() { return a() == 8 ? 0 : 1; }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo -Wl,-rpath,@loader_path/sub
$RUN $t/exe
otool -L $t/exe > $t/libs
grep -A2 libSystem $t/libs | grep -q '@rpath/libw.dylib (compatibility version 3.0.0, current version 3.2.1)'
grep -A2 libSystem $t/libs | grep -q 'libz'
not grep -q libfoo $t/libs
dyld_info -fixups $t/exe | grep -q 'libw/_w'

# The library itself is never read: one that isn't there is no error.
rm $t/sub/libw.dylib
$CC --ld-path=$mold -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
otool -L $t/exe2 | grep -q '@rpath/libw.dylib'

# Named on the command line, a library is loaded as named.
$CC -shared -o $t/sub/libw.dylib $t/w.o -Wl,-install_name,@rpath/libw.dylib
$CC --ld-path=$mold -o $t/exe3 $t/main.o -L$t -Wl,-merge-lfoo -L$t/sub -Wl,-weak-lw
otool -L $t/exe3 | grep -q '@rpath/libw.dylib (compatibility version 0.0.0, current version 0.0.0, weak)'
dyld_info -fixups $t/exe3 | grep -q 'libw/_w \[weak-import\]'
