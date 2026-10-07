#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime writes a non-external symbol's name cut at its first ".llvm.",
# which ThinLTO appends with a hash of the module to the statics it
# promotes, in the symbol table and the debug notes alike, so that the
# debugger shows the source's name. A global keeps its whole name, and
# the map shows every name whole.
cat <<'EOF2' | $CC -O1 -g -c -xc - -o $t/a.o
__attribute__((visibility("hidden"))) int hid(int x) __asm__("_hid.llvm.123");
__attribute__((visibility("hidden"))) int hid(int x) { return x + 1; }
__attribute__((noinline)) static int st(int x) __asm__("_st.llvm.4.llvm.5");
__attribute__((noinline)) static int st(int x) { return x + 2; }
int glob(int x) __asm__("_glob.llvm.789");
int glob(int x) { return x + 3; }
int main(int c, char **v) { return hid(c) + st(c) + glob(c) == 3 * c + 6 ? 0 : 1; }
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-map,$t/map
$RUN $t/exe
nm -ap $t/exe > $t/nm
grep -q ' t _hid$' $t/nm
grep -q ' t _st$' $t/nm
grep -q ' FUN _hid$' $t/nm
grep -q ' T _glob.llvm.789$' $t/nm
not grep -q -e '_hid\.' -e '_st\.' $t/nm
grep -q '\] _hid.llvm.123$' $t/map

# The same goes for a -r output's non-external symbols.
$mold -arch $ARCH -r -o $t/r.o $t/a.o
nm -ap $t/r.o > $t/nm2
grep -q ' t _hid$' $t/nm2
grep -q ' t _st$' $t/nm2
