#!/bin/bash
source "$(dirname "$0")"/common.inc

# A final image of code LTO compiled is dead-stripped, -dead_strip or
# not: ld-prime walks the atoms as dead stripping does to find what
# libLTO must preserve, every bitcode file's code a root, and then
# strips what the walk didn't reach. So native code nothing live calls
# is gone, and with it its references - to an undefined symbol, which
# is then no error, or to a hidden bitcode function, which LTO need not
# keep. An import only stripped code used stays in the symbol table,
# unbound, which -dead_strip drops; the map lists nothing stripped.
cat <<EOF | $CC -O1 -flto=thin -c -xc - -o $t/a.o
int used_native(int);
int main(int argc, char **argv) { return used_native(argc) == 3 ? 0 : 1; }
__attribute__((visibility("hidden"))) int helper(int x) { return x + 7; }
EOF
cat <<EOF | $CC -O1 -c -xc - -o $t/b.o
#include <unistd.h>
int nowhere(int);
int helper(int);
int used_native(int x) { return x + 2; }
__attribute__((visibility("hidden"))) int dead_native(int x) {
  return nowhere(x) + helper(x) + getpid();
}
int exported_native(int x) { return x; }
EOF

$CC --ld-path=$mold -flto -o $t/exe $t/a.o $t/b.o -Wl,-map,$t/map
$t/exe
nm -m $t/exe > $t/nm
not grep -q -e _dead_native -e _helper -e _nowhere -e _exported_native $t/nm
grep -q '(undefined) external _getpid (from libSystem)' $t/nm
dyld_info -fixups $t/exe > $t/fixups
not grep -q _getpid $t/fixups
not grep -q 'Dead Stripped' $t/map

$CC --ld-path=$mold -flto -o $t/exe2 $t/a.o $t/b.o -Wl,-dead_strip -Wl,-map,$t/map2
$t/exe2
nm -m $t/exe2 > $t/nm2
not grep -q -e _dead_native -e _getpid $t/nm2
grep -q 'Dead Stripped' $t/map2

# A dylib keeps its exports, and loses the rest nothing uses.
$CC --ld-path=$mold -flto -shared -o $t/libfoo.dylib $t/a.o $t/b.o
nm -m $t/libfoo.dylib > $t/nm3
grep -q 'external _exported_native' $t/nm3
not grep -q -e _dead_native -e _nowhere $t/nm3

# Without bitcode in the link, nothing is stripped unasked.
cat <<EOF | $CC -O1 -c -xc - -o $t/c.o
int used_native(int);
int helper(int x) { return x + 7; }
int nowhere(int x) { return x; }
int main(int argc, char **argv) { return used_native(argc) == 3 ? 0 : 1; }
EOF
$CC --ld-path=$mold -o $t/exe4 $t/c.o $t/b.o
nm -m $t/exe4 | grep -q _dead_native
