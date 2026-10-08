#!/bin/bash
source "$(dirname "$0")"/common.inc

# A C++ function's unwind info names its personality routine, which is
# no relocation of the code but an unwind record's (or a CIE's) field.
# Linked without libc++ under -undefined dynamic_lookup, the personality
# is looked up at run time like any other undefined symbol; without
# it, it is reported undefined.
cat <<EOF | $CXX -o $t/a.o -c -xc++ -
void g();
struct S { ~S(); };
int f() { S s; g(); return 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -undefined dynamic_lookup
dyld_info -fixups $t/exe > $t/fixups
grep -q 'flat-namespace>/___gxx_personality_v0' $t/fixups

not $CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o 2> $t/log
grep -q '___gxx_personality_v0' $t/log
