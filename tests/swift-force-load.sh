#!/bin/bash
source "$(dirname "$0")"/common.inc

# The Swift compiler keeps an overlay loaded with a weak reference to
# its __swift_FORCE_LOAD_$_<overlay> marker, stored in a hidden
# __DATA,__const word. ld-prime lists the dylib as a dependency (weak
# when every reference to it is weak) but writes no fixup for the
# word, which stays zero; other weak imports bind as usual.
cat <<EOF | $CC -o $t/lib.o -c -xc -
int fl asm("__swift_FORCE_LOAD_\$_swiftFoo") = 1;
int other = 2;
EOF
$CC --ld-path=$mold -dynamiclib -o $t/libfoo.dylib $t/lib.o -install_name @rpath/libfoo.dylib
nm $t/libfoo.dylib | grep 'D __swift_FORCE_LOAD_\$_swiftFoo'

cat <<EOF | $CC -o $t/a.o -c -xc -
extern int fl asm("__swift_FORCE_LOAD_\$_swiftFoo") __attribute__((weak_import));
extern int other __attribute__((weak_import));
__attribute__((visibility("hidden"), used))
int *ref asm("__swift_FORCE_LOAD_\$_swiftFoo_\$_M") = &fl;
__attribute__((visibility("hidden"), used)) int *op = &other;
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o $t/libfoo.dylib -Wl,-rpath,$t
$t/exe
dyld_info -fixups $t/exe > $t/fixups
not grep -q FORCE_LOAD $t/fixups
grep -q 'libfoo/_other \[weak-import\]' $t/fixups
otool -L $t/exe | grep 'libfoo.dylib.*weak'
nm -m $t/exe | grep 'undefined) weak external __swift_FORCE_LOAD_\$_swiftFoo (from libfoo)'

# With classic dyld info too.
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/libfoo.dylib -Wl,-rpath,$t -Wl,-no_fixup_chains
$t/exe2
dyld_info -fixups $t/exe2 > $t/fixups2
not grep -q FORCE_LOAD $t/fixups2
grep -q 'libfoo/_other \[weak-import\]' $t/fixups2
