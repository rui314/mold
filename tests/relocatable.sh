#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
static const char msg[] = "from a";
const char *get_msg() { return msg; }
int helper() { return 7; }
EOF2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
int helper();
int wrapped() { return helper() + 1; }
EOF2

# Merge the two objects into one relocatable object with our linker...
$mold -r -arch $ARCH -platform_version macos 15.0 15.0 -o $t/merged.o $t/a.o $t/b.o
otool -h $t/merged.o | grep '	1	' || otool -hv $t/merged.o | grep OBJECT

cat <<EOF2 | $CC -o $t/main.o -c -xc -
#include <stdio.h>
const char *get_msg();
int wrapped();
int main() { printf("%s %d\n", get_msg(), wrapped()); }
EOF2

# ...then use Apple's toolchain for the final link, proving the merged
# object is a valid input for other linkers, and ours too. Apple's ld
# does not ad-hoc sign x86_64 output, and the hosted CI runner hangs
# in exec (unkillably) on an unsigned binary under Rosetta, so ask it
# to sign, as it does for arm64 anyway.
$CC -Wl,-adhoc_codesign -o $t/exe1 $t/main.o $t/merged.o
$t/exe1 | grep '^from a 8$'

$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/merged.o
$t/exe2 | grep '^from a 8$'

# Unwind info survives the merge: C++ exceptions still work after -r.
cat <<EOF2 | $CXX -o $t/e1.o -c -xc++ -
#include <cstdio>
void thrower() { throw 42; }
EOF2
cat <<EOF2 | $CXX -o $t/e2.o -c -xc++ -
#include <cstdio>
void thrower();
int main() {
  try { thrower(); } catch (int e) { printf("caught %d\n", e); }
}
EOF2

$mold -r -arch $ARCH -platform_version macos 15.0 15.0 -o $t/exc.o $t/e1.o $t/e2.o
# ld-prime keeps the exception table after the other __TEXT sections.
otool -l $t/exc.o | awk '$1 == "sectname" {print $2}' | tr '\n' ' ' | grep '__cstring __gcc_except_tab'
$CXX --ld-path=$mold -o $t/exc1 $t/exc.o
$t/exc1 | grep 'caught 42'
$CXX -Wl,-adhoc_codesign -o $t/exc2 $t/exc.o
$t/exc2 | grep 'caught 42'
