#!/bin/bash
source "$(dirname "$0")"/common.inc

# Auto-link requests (LC_LINKER_OPTION) are not acted on by a -r link:
# ld64 copies them into the output object and the final link resolves
# them. Loading the libraries during -r would let them claim symbols
# the output must leave undefined.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <zlib.h>
const char *ver(void) { return zlibVersion(); }
EOF
cat <<EOF | $CC -o $t/b.o -c -x assembler -
.linker_option "-lz"
.linker_option "-framework", "Foundation"
EOF
cat <<EOF | $CC -o $t/c.o -c -x assembler -
.linker_option "-lz"
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
const char *ver(void);
int main() { printf("%s\n", ver()); }
EOF

# A dylib named on the -r command line is ignored, with a warning.
$mold -r -arch $ARCH -syslibroot "$(xcrun --show-sdk-path)" -o $t/r.o \
  $t/a.o $t/b.o $t/c.o -lSystem > $t/log 2>&1
grep -q 'ignoring unexpected dylib' $t/log

# b.o and c.o lack MH_SUBSECTIONS_VIA_SYMBOLS, so the output does too.
otool -h $t/r.o | tail -1 | grep ' 0x00000000$'

# The zlib reference stays undefined, and each distinct option appears
# once.
nm -m $t/r.o | grep 'undefined.*_zlibVersion'
otool -l $t/r.o > $t/lc
[ "$(grep -c LC_LINKER_OPTION $t/lc)" = 2 ]
grep -q -- '-lz' $t/lc
grep -q Foundation $t/lc
# The load commands come in ld64's order, and without -platform_version
# the build version is the first object's.
[ "$(grep '^ *cmd ' $t/lc | awk '{print $2}' | uniq | tr '\n' ' ')" = "LC_SEGMENT_64 LC_SYMTAB LC_BUILD_VERSION LC_DATA_IN_CODE LC_LINKER_OPTION " ]
grep -A5 LC_BUILD_VERSION $t/lc | grep "minos $(otool -l $t/a.o | grep minos | awk '{print $2}')"
grep -A5 LC_BUILD_VERSION $t/lc | grep "sdk $(otool -l $t/a.o | grep ' sdk ' | awk '{print $2}')"

# The final link auto-links libz from the carried option.
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o
$t/exe | grep '^1\.'

# The options go through as they are, in input order and each only
# once, so that the final link reads them as it would the inputs'.
# (ld-prime rewrites them: one per library, the libraries first, each
# kind sorted by name, losing -force_load and a framework's ",suffix".)
cat <<EOF2 | $CC -o $t/d.o -c -x assembler -
.linker_option "-lzzz"
.linker_option "-needed-lfoo"
.linker_option "-lfoo"
.linker_option "-hidden-lbar"
.linker_option "-lazy-lqux"
.linker_option "-lzzz"
.linker_option "-framework", "Foo"
.linker_option "-needed_framework", "Foo"
.linker_option "-framework", "Bar,_debug"
.linker_option "-force_load", "/p/libx.a"
.linker_option "-needed_library", "/p/liby.dylib"
EOF2
$mold -r -arch $ARCH -o $t/r2.o $t/d.o
otool -l $t/r2.o | awk '$2 == "LC_LINKER_OPTION" { n++ }
  /^ *string/ { s[n] = s[n] (s[n] == "" ? "" : " ") $3 }
  END { for (i = 1; i <= n; i++) print s[i] }' > $t/lc2
cat > $t/lc2.expected <<EOF2
-lzzz
-needed-lfoo
-lfoo
-hidden-lbar
-lazy-lqux
-framework Foo
-needed_framework Foo
-framework Bar,_debug
-force_load /p/libx.a
-needed_library /p/liby.dylib
EOF2
diff $t/lc2.expected $t/lc2

# So a -force_load carried through has the final link load an archive
# member nothing refers to.
echo 'int forced = 42;' | $CC -o $t/forced.o -c -xc -
rm -f $t/libforced.a
ar rcs $t/libforced.a $t/forced.o
cat <<EOF2 | $CC -o $t/e.o -c -x assembler -
.linker_option "-force_load", "$t/libforced.a"
EOF2
echo 'int main() { return 0; }' | $CC -o $t/main2.o -c -xc -
$mold -r -arch $ARCH -o $t/r3.o $t/main2.o $t/e.o
$CC --ld-path=$mold -o $t/exe3 $t/r3.o
nm $t/exe3 | grep -q ' D _forced$'
