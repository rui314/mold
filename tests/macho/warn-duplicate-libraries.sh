#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF
rm -f $t/libfoo.a; ar rcs $t/libfoo.a $t/a.o

cat <<EOF | $CC -o $t/main.o -c -xc -
void foo();
int main() { foo(); }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -lfoo 2> $t/log
grep -q "ignoring duplicate libraries: '.*foo'" $t/log

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -lfoo \
  -Wl,-no_warn_duplicate_libraries 2> $t/log2
! grep -q 'duplicate libraries' $t/log2 || false

# One warning lists every library given more than once; the same
# library under another kind of option is no repeat.
cat <<EOF | $CC -o $t/b.o -c -xc -
void bar() {}
EOF
rm -f $t/libbar.a; ar rcs $t/libbar.a $t/b.o
$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -lbar -lfoo -lbar -lfoo 2> $t/log3
[ "$(grep -c 'duplicate libraries' $t/log3)" = 1 ]
grep -q "ignoring duplicate libraries: '.*bar', '.*foo'" $t/log3

$CC --ld-path=$mold -o $t/exe $t/main.o -L$t -lfoo -Wl,-hidden-lfoo 2> $t/log4
not grep -q 'duplicate libraries' $t/log4

# So does a library named by path twice alike, and an archive's bare
# path, but not a dylib's or a framework option.
$CC --ld-path=$mold -shared -o $t/libbaz.dylib $t/a.o
$CC --ld-path=$mold -o $t/exe $t/main.o $t/libfoo.a $t/libfoo.a $t/libbaz.dylib \
  $t/libbaz.dylib -Wl,-weak_library,$t/libbaz.dylib,-weak_library,$t/libbaz.dylib \
  -Wl,-force_load,$t/libfoo.a,-force_load,$t/libfoo.a -framework CoreFoundation \
  -framework CoreFoundation 2> $t/log5
grep 'ignoring duplicate libraries' $t/log5 > $t/dups5
grep -qF "$t/libfoo.a'" $t/dups5
grep -qF "$t/libbaz.dylib'" $t/dups5
not grep -q CoreFoundation $t/dups5
