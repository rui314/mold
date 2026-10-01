#!/bin/bash
source "$(dirname "$0")"/common.inc

# A function that deduplication folded into an identical one has no
# bytes of its own, but a mergeable dylib's record keeps its name, as
# ld-prime records it: an alias of the function kept, after the
# imports, which its other labels are aliases of in turn. A merging
# link finds an exported Swift function folded so (Swift promises no
# function an address of its own), and the image has every name.
if [ $ARCH = arm64 ]; then
  body() { echo "mov w0, #$1"; echo ret; }
else
  body() { echo "movl \$$1, %eax"; echo ret; echo nop; echo nop; }
fi

{
  echo '.subsections_via_symbols'
  echo '.text'
  echo '.globl "_$s2a", "_$s2b"'
  echo '"_$s2a":'; body 2
  echo '"_$s2b":'; body 2
} > $t/a.s
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/b.o -c -O1 -xc -
__attribute__((visibility("hidden"), noinline)) int h1(int x) { return x * 7 + 3; }
__attribute__((visibility("hidden"), noinline)) int h2(int x) { return x * 7 + 3; }
int call(int x) { return h1(x) + h2(x + 1); }
EOF

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int s2a(void) __asm__("_\$s2a");
int s2b(void) __asm__("_\$s2b");
int call(int);
int main() { printf("%d %d %d\n", s2a(), s2b(), call(1)); }
EOF

# ld-prime folds at -O1 and up or with -deduplicate.
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o $t/b.o -Wl,-make_mergeable \
  -Wl,-deduplicate -Wl,-install_name,@rpath/libfoo.dylib
nm $t/libfoo.dylib > $t/syms
[ "$(grep -c '_\$s2[ab]$' $t/syms)" = 2 ]
[ "$(grep '_\$s2[ab]$' $t/syms | cut -d' ' -f1 | uniq | wc -l)" -eq 1 ]
[ "$(grep -E ' _h[12]$' $t/syms | cut -d' ' -f1 | uniq | wc -l)" -eq 1 ]

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo
$t/exe | grep -q '^2 2 27$'
nm $t/exe > $t/exe-syms
grep -q ' _h2$' $t/exe-syms

$CC -o $t/exe2 $t/main.o -L$t -Wl,-merge-lfoo
$t/exe2 | grep -q '^2 2 27$'
otool -L $t/exe2 > $t/libs
not grep -q libfoo $t/libs

# A weak definition's losing copy has no entry, though the winner it
# coalesced with is a folded function: the name has one.
cat <<EOF | $CXX -o $t/c.o -c -O1 -xc++ -
template <int N> struct T { __attribute__((noinline)) static int f(int x) { return x * 13 + 1; } };
extern "C" int fc(int x) { return T<1>::f(x) + T<2>::f(x + 1); }
EOF
cat <<EOF | $CXX -o $t/d.o -c -O1 -xc++ -
template <int N> struct T { __attribute__((noinline)) static int f(int x) { return x * 13 + 1; } };
extern "C" int fd(int x) { return T<2>::f(x) + T<3>::f(x + 2); }
EOF
cat <<EOF | $CC -o $t/main2.o -c -xc -
#include <stdio.h>
int fc(int), fd(int);
int main() { printf("%d %d\n", fc(1), fd(2)); }
EOF
$CXX --ld-path=$mold -shared -o $t/libbar.dylib $t/c.o $t/d.o -Wl,-make_mergeable \
  -Wl,-deduplicate -Wl,-install_name,@rpath/libbar.dylib

# The names of the record's entries.
python3 - $t/libbar.dylib > $t/names <<'EOF'
import struct, sys
data = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', data, 16)[0]):
    cmd, size, dataoff = struct.unpack_from('<III', data, off)
    if cmd == 0x36:
        b = data[dataoff:]
    off += size
nents, count = struct.unpack_from('<II', b, 0x60)
names, _ = struct.unpack_from('<II', b, 0x88)
for i in range(count):
    name = struct.unpack_from('<I', b, nents + 40 * i + 12)[0]
    if name != 0xffffff:
        at = names + 16 * name
        start = at + struct.unpack_from('<q', b, at)[0]
        print(b[start:b.index(b'\0', start)].decode())
EOF
[ "$(grep -c '^__ZN1TILi2EE1fEi$' $t/names)" = 1 ]
[ "$(grep -c '^__ZN1TILi3EE1fEi$' $t/names)" = 1 ]

$CC --ld-path=$mold -o $t/exe3 $t/main2.o -L$t -Wl,-merge-lbar
$t/exe3 | grep -q '^41 80$'
$CC -o $t/exe4 $t/main2.o -L$t -Wl,-merge-lbar
$t/exe4 | grep -q '^41 80$'
