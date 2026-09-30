#!/bin/bash
source "$(dirname "$0")"/common.inc

# A library named more than once takes on what every naming says, in
# any order: a -needed_* naming keeps its load command under
# -dead_strip_dylibs, a -weak_* one makes its imports weak, and a
# -reexport_* one re-exports it. Foundation's stub brings CoreFoundation
# in as a public re-export, which a later naming names again; the
# weakness it inherits from Foundation gives way to its first
# command-line naming, though not to an auto-link option (ld-prime).
cat <<EOF | $CC -o $t/cf.o -c -xc -
#include <CoreFoundation/CoreFoundation.h>
int main() { return CFGetRetainCount(0) ? 0 : 0; }
EOF
echo 'int main() { return 0; }' | $CC -o $t/m.o -c -xc -
echo '.linker_option "-framework", "CoreFoundation"' | $CC -o $t/al.o -c -xassembler -

cf_cmd() {
  otool -l $1 | awk '$1 == "cmd" { c = $2 }
    $1 == "name" && $2 ~ /CoreFoundation.framework/ { print c }'
}
cf_weak() { nm -m $1 | grep _CFGetRetainCount | grep -c weak; }

$CC --ld-path=$mold -o $t/exe1 $t/m.o -framework Foundation \
  -Wl,-needed_framework,CoreFoundation -Wl,-dead_strip_dylibs
[ "$(cf_cmd $t/exe1)" = LC_LOAD_DYLIB ]

$CC --ld-path=$mold -o $t/exe2 $t/m.o -framework CoreFoundation \
  -Wl,-needed_framework,CoreFoundation -Wl,-dead_strip_dylibs
[ "$(cf_cmd $t/exe2)" = LC_LOAD_DYLIB ]

$CC --ld-path=$mold -o $t/exe3 $t/cf.o -framework CoreFoundation \
  -Wl,-weak_framework,CoreFoundation
[ "$(cf_cmd $t/exe3)" = LC_LOAD_WEAK_DYLIB ]
[ "$(cf_weak $t/exe3)" = 1 ]

$CC --ld-path=$mold -o $t/exe4 $t/cf.o -Wl,-weak_framework,Foundation \
  -framework CoreFoundation
[ "$(cf_cmd $t/exe4)" = LC_LOAD_DYLIB ]
[ "$(cf_weak $t/exe4)" = 0 ]

$CC --ld-path=$mold -o $t/exe5 $t/cf.o $t/al.o -Wl,-weak_framework,Foundation
[ "$(cf_cmd $t/exe5)" = LC_LOAD_WEAK_DYLIB ]
[ "$(cf_weak $t/exe5)" = 1 ]

$CC --ld-path=$mold -o $t/b.dylib -shared $t/cf.o -framework CoreFoundation \
  -Wl,-reexport_framework,CoreFoundation
[ "$(cf_cmd $t/b.dylib)" = LC_REEXPORT_DYLIB ]
