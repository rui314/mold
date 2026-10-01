#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib records which object each subsection came from and
# what its debug notes said, so the merging image's notes (N_SO, N_OSO
# and the symbols') point a debugger at those objects, as a link of the
# objects would.
cat <<EOF | $CC -o $t/a.o -c -g -O1 -xc -
int counter = 5;
static int hidden;
int foo(int x) { hidden++; return x + counter + hidden; }
EOF
cat <<EOF | $CC -o $t/b.o -c -g -O1 -xc -
int foo(int);
int bar(int x) { return foo(x) * 2; }
EOF
cat <<EOF | $CC -o $t/main.o -c -g -O1 -xc -
int bar(int);
int main() { return bar(1) == 14 ? 0 : 1; }
EOF

$CC -shared -o $t/libfoo.dylib $t/a.o $t/b.o -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/libfoo.dylib
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -Wl,-merge-lfoo
$t/exe

nm -ap $t/exe > $t/syms
grep -q " OSO .*/$t/a.o$" $t/syms
grep -q " OSO .*/$t/b.o$" $t/syms
grep -q ' FUN _foo$' $t/syms
grep -q ' FUN _bar$' $t/syms
grep -q ' GSYM _counter$' $t/syms
grep -q ' STSYM _hidden$' $t/syms

# The notes of each object follow its N_OSO.
sed -n '/OSO .*a\.o$/,/OSO .*b\.o$/p' $t/syms > $t/a-notes
grep -q ' FUN _foo$' $t/a-notes
not grep -q ' FUN _bar$' $t/a-notes
