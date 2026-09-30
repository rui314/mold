#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -static image can use no dylib, having no dyld to load one: -l
# looks for archives only, even under -search_dylibs_first, and a dylib
# named outright is ignored with a warning (ld-prime).
cat <<EOF | $CC -o $t/foo.o -c -xc -
int foo(void) { return 1; }
EOF
rm -f $t/libfoo.a
ar rcs $t/libfoo.a $t/foo.o
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/foo.o -Wl,-install_name,@rpath/libfoo.dylib
$CC --ld-path=$mold -shared -o $t/libbar.dylib $t/foo.o -Wl,-install_name,@rpath/libbar.dylib

cat <<EOF | $CC -o $t/main.o -c -xc -
int foo(void);
void _start(void) { foo(); }
EOF

$mold -arch $ARCH -static -e __start $t/main.o -L$t -lfoo -o $t/exe
otool -L $t/exe > $t/libs
not grep -q libfoo $t/libs
nm -m $t/exe | grep -q '(__TEXT,__text) external _foo'

$mold -arch $ARCH -static -e __start $t/main.o -L$t -search_dylibs_first -lfoo -o $t/exe2
nm -m $t/exe2 | grep -q '(__TEXT,__text) external _foo'

not $mold -arch $ARCH -static -e __start $t/main.o -L$t -lbar -o $t/exe3 2> $t/log3
grep -q bar $t/log3

$mold -arch $ARCH -static -e __start $t/main.o $t/libbar.dylib $t/libfoo.a -o $t/exe4 2> $t/log4
grep -q "ignoring unexpected dylib '.*libbar.dylib'" $t/log4
nm -m $t/exe4 | grep -q '(__TEXT,__text) external _foo'

# So does a kext, which kmutil links into the kernel. A relocatable
# object searches for dylibs as usual, even with -static, which does
# not apply to it, but ignores what it finds with the same warning.
$mold -arch $ARCH -kext $t/main.o -L$t -lfoo -o $t/kext
nm -m $t/kext | grep -q '(__TEXT.*,__text) external _foo'
not $mold -arch $ARCH -kext $t/main.o -L$t -lbar -o $t/kext2 2> $t/log5
grep -q bar $t/log5
$mold -arch $ARCH -kext $t/main.o $t/libbar.dylib -o $t/kext3 2> $t/log6
grep -q "ignoring unexpected dylib '.*libbar.dylib'" $t/log6
otool -L $t/kext3 > $t/libs3
not grep -q libbar $t/libs3

$mold -arch $ARCH -r -static $t/main.o -L$t -lbar -o $t/r.o 2> $t/log7
grep -q "ignoring unexpected dylib '.*libbar.dylib'" $t/log7
$mold -arch $ARCH -r -static $t/main.o -L$t -lfoo -o $t/r2.o 2> $t/log8
grep -q "ignoring unexpected dylib '.*libfoo.dylib'" $t/log8
nm -m $t/r2.o | grep -q 'undefined.* _foo'
