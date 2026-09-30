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
