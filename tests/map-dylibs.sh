#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime's map lists every dylib the command line names, where it
# names it and by the path it gives, whether -dead_strip_dylibs drops it
# or not, and whether a library another one re-exports loaded it first
# (Foundation's stub re-exports libobjc, which -lobjc names later).
cat <<EOF | $CC -o $t/a.o -c -xc -
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -lobjc -lz \
  -Wl,-dead_strip_dylibs -Wl,-map,$t/map
otool -L $t/exe > $t/libs
not grep -q libz $t/libs
grep -Eq '^\[  2\] .*/Foundation.framework/Foundation.tbd$' $t/map
grep -Eq '^\[  3\] .*/usr/lib/libobjc.tbd$' $t/map
grep -Eq '^\[  4\] .*/usr/lib/libz.tbd$' $t/map

# The same goes for the libraries auto-link options name, which come
# last, in the order ld-prime acts on the options: sorted.
cat <<EOF2 | $CC -o $t/b.o -c -xc -
void NSLog(void *, ...);
void *objc_autoreleasePoolPush(void);
int main() { objc_autoreleasePoolPush(); NSLog(0); return 0; }
EOF2
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.linker_option "-lobjc"
.linker_option "-framework", "Foundation"
EOF2
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/c.o -Wl,-map,$t/map2
grep -A2 'Foundation.framework/Foundation.tbd$' $t/map2 | grep -q '/usr/lib/libobjc.tbd$'

# With lazy binding (no chained fixups), ld-prime wants libSystem's
# dyld_stub_binder whether or not a stub needs it, so the map lists the
# library that exports it, libdyld; not so under -bind_at_load.
cat <<EOF2 | $CC -o $t/d.o -c -xc - -mmacosx-version-min=11.0
int main() { return 0; }
EOF2
$CC --ld-path=$mold -o $t/exe3 $t/d.o -mmacosx-version-min=11.0 -Wl,-map,$t/map3
grep -Eq '^\[  3\] .*/system/libdyld.tbd$' $t/map3
$CC --ld-path=$mold -o $t/exe4 $t/d.o -mmacosx-version-min=11.0 -Wl,-bind_at_load \
  -Wl,-map,$t/map4
not grep -q libdyld $t/map4
not grep -q libdyld $t/map

# ld-prime lists a dylib once for each input naming it, by the path it
# gives - another file with its install name (libgcc_s.1.tbd, an alias
# of libSystem.tbd) or the same file again -, used or not; the symbols
# bound to it are the first one's.
echo 'int foo(void) { return 1; }' | $CC -shared -xc - -o $t/libfoo.dylib
ln -sf libfoo.dylib $t/libbar.dylib
cat <<EOF2 | $CC -o $t/e.o -c -xc -
int foo(void);
int main() { return foo(); }
EOF2
$CC --ld-path=$mold -o $t/exe5 $t/e.o $t/libbar.dylib $t/libfoo.dylib $t/libfoo.dylib \
  -lgcc_s.1 -Wl,-dead_strip_dylibs -Wl,-map,$t/map5
sed -n '/^# Object files:/,/^# Sections:/p' $t/map5 > $t/files5
grep -Fq "[  2] $t/libbar.dylib" $t/files5
grep -Fq "[  3] $t/libfoo.dylib" $t/files5
grep -Fq "[  4] $t/libfoo.dylib" $t/files5
grep -Eq '^\[  [0-9]\] .*/usr/lib/libgcc_s.1.tbd$' $t/files5
grep -Eq $'\t\\[  2\\] _foo.stub$' $t/map5
