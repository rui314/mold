#!/bin/bash
source "$(dirname "$0")"/common.inc

# From macOS 27, a dylib named by -lazy-l, -lazy_library or
# -lazy_framework loads at the first use of one of its symbols. It has
# no LC_LOAD_DYLIB but an LC_LAZY_LOAD_DYLIB_INFO record; calls go
# through a helper per symbol ($lazyLoadStub) and GOT loads through
# load helpers, which have __dyld_lazy_load load the dylib and bind
# its __lazy_load_got slots, then go on through the slots.
sdk=$(xcrun --show-sdk-path)
grep -q __dyld_lazy_load "$sdk/usr/lib/system/libdyld.tbd" || skip

cat <<EOF | $CC -o $t/foo.o -c -xc -
#include <stdio.h>
int fdata = 5;
int foo(void) { return 3; }
int bar(int x) { return x + 1; }
__attribute__((constructor)) static void init(void) { printf("foo loaded\n"); }
EOF
cat <<EOF | $CC -o $t/qux.o -c -xc -
#include <stdio.h>
int qux(void) { return 4; }
__attribute__((constructor)) static void init(void) { printf("qux loaded\n"); }
EOF
$CC -o $t/libfoo.dylib -shared $t/foo.o -Wl,-install_name,@rpath/libfoo.dylib
$CC -o $t/libqux.dylib -shared $t/qux.o -Wl,-install_name,@rpath/libqux.dylib

# leaf() saves no link register, so its GOT load branches to a helper
# of its own, which branches back.
cat <<EOF | $CC -o $t/a.o -c -xc - -O1 -mmacosx-version-min=27.0
#include <stdio.h>
extern int fdata;
int foo(void), bar(int), qux(void);
__attribute__((noinline)) int leaf(void) { return fdata; }
int main() {
  printf("start\n");
  int n = leaf();
  printf("%d %d %d\n", n, foo(), bar(4));
  printf("%d\n", qux());
}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -Wl,-lazy-lfoo,-lazy_library,$t/libqux.dylib \
  -Wl,-rpath,$t -mmacosx-version-min=27.0
otool -l $t/exe > $t/lc
[ "$(grep -c 'cmd LC_LAZY_LOAD_DYLIB_INFO' $t/lc)" = 2 ]
not grep -q libfoo $t/lc
grep -q 'sectname __lazy_helpers' $t/lc
grep -q 'sectname __lazy_load_got' $t/lc
nm -m $t/exe > $t/nm
grep -q '(undefined) external __dyld_lazy_load (from libSystem)' $t/nm
grep -q 'non-external (was a private external) _foo$lazyLoadStub' $t/nm
grep -q '(__DATA,__lazy_load_got) non-external _foo$lazyGOT' $t/nm
grep -q '(__DATA,__data) non-external _lazyLoadFlag$libfoo.dylib' $t/nm
if [ $ARCH = arm64 ]; then
  grep -q '_fdata$lazyGOT$loadHelper_x8$for$_leaf+0' $t/nm
else
  grep -q '_fdata$lazyGOT$loadHelper_rax' $t/nm
fi

# Each dylib's record: its install name, the addresses of its flag word
# and of its chain's first __lazy_load_got slot, the chain's pointer
# format (DYLD_CHAINED_PTR_64_OFFSET, not weak), and the symbols the
# program uses, which the chain binds in turn. Records and symbols may
# come in any order.
python3 - $t/exe > $t/recs <<'EOF'
import struct, sys
d = open(sys.argv[1], 'rb').read()
off = 32
for _ in range(struct.unpack_from('<I', d, 16)[0]):
    cmd, size = struct.unpack_from('<II', d, off)
    if cmd == 0x19 and d[off + 8:off + 24].rstrip(b'\0') == b'__TEXT':
        base = struct.unpack_from('<Q', d, off + 24)[0]
    if cmd == 0x3a:
        rec = struct.unpack_from('<II', d, off + 8)[0]
        name, flag, fmt, chain, n, arr = struct.unpack_from('<6I', d, rec)
        cstr = lambda o: d[rec + o:d.index(b'\0', rec + o)].decode()
        syms = sorted({cstr(struct.unpack_from('<I', d, rec + arr + 4 * i)[0]) for i in range(n)})
        print(cstr(name), ' '.join(syms), hex(fmt), hex(base + flag), hex(base + chain))
    off += size
EOF
nm $t/exe > $t/syms
addrs() {
  awk -v s="$1" '$3 == s { print $1 }' $t/syms | while read a; do printf '0x%x\n' 0x$a; done
}
grep -q "^@rpath/libfoo.dylib _bar _fdata _foo 0x60000 $(addrs '_lazyLoadFlag$libfoo.dylib') " $t/recs
grep -q "^@rpath/libqux.dylib _qux 0x60000 $(addrs '_lazyLoadFlag$libqux.dylib') " $t/recs
foo_chain=$(awk '$1 == "@rpath/libfoo.dylib" { print $NF }' $t/recs)
qux_chain=$(awk '$1 == "@rpath/libqux.dylib" { print $NF }' $t/recs)
for s in _bar _fdata _foo; do addrs "$s\$lazyGOT"; done > $t/foo-slots
grep -qx $foo_chain $t/foo-slots
addrs '_qux$lazyGOT' > $t/qux-slots
grep -qx $qux_chain $t/qux-slots

# The dylibs load as the program first uses them.
$RUN $t/exe > $t/out
printf 'start\nfoo loaded\n5 3 5\nqux loaded\n4\n' | cmp - $t/out

# Only calls and GOT loads can be lazy; a pointer in data is refused.
cat <<EOF | $CC -o $t/b.o -c -xc -
extern int fdata;
int *p = &fdata;
int main() { return *p; }
EOF
not $CC --ld-path=$mold -o $t/exe2 $t/b.o -L$t -Wl,-lazy-lfoo -mmacosx-version-min=27.0 2> $t/log
grep -q "use of '_fdata' in '_p' cannot be lazy loaded." $t/log

# Each such use is refused.
cat <<EOF | $CC -o $t/b2.o -c -xc -
extern int fdata;
int foo(void);
int *p = &fdata;
void *q = (void *)foo;
int main() { return *p; }
EOF
not $CC --ld-path=$mold -o $t/exe2 $t/b2.o -L$t -Wl,-lazy-lfoo -mmacosx-version-min=27.0 2> $t/log
grep -q "use of '_fdata' in '_p' cannot be lazy loaded.$" $t/log
grep -q "use of '_foo' in '_q' cannot be lazy loaded.$" $t/log

# A dylib can load one lazily too.
cat <<EOF | $CC -o $t/c.o -c -xc -
int foo(void);
int mid(void) { return foo() + 1; }
EOF
$CC --ld-path=$mold -o $t/libmid.dylib -shared $t/c.o -L$t -Wl,-lazy-lfoo \
  -Wl,-install_name,@rpath/libmid.dylib -mmacosx-version-min=27.0
cat <<EOF | $CC -o $t/d.o -c -xc -
#include <stdio.h>
int mid(void);
int main() { printf("start\n"); printf("%d\n", mid()); }
EOF
$CC --ld-path=$mold -o $t/exe3 $t/d.o -L$t -lmid -Wl,-rpath,$t -mmacosx-version-min=27.0
$RUN $t/exe3 > $t/out3
printf 'start\nfoo loaded\n4\n' | cmp - $t/out3

# A lazy dylib the program does not use has no record and no load
# command.
cat <<EOF | $CC -o $t/e.o -c -xc -
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe4 $t/e.o -L$t -Wl,-lazy-lfoo -mmacosx-version-min=27.0
otool -l $t/exe4 > $t/lc4
not grep -q LC_LAZY_LOAD_DYLIB_INFO $t/lc4
not grep -q libfoo $t/lc4
$RUN $t/exe4
