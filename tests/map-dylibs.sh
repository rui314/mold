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
