#!/bin/bash
source "$(dirname "$0")"/common.inc

# The default library search path is ld64's /usr/lib and /usr/local/lib
# with ld-prime's /usr/lib/swift between them, searched for any library;
# the frameworks' is /Library/Frameworks, then /System/Library/Frameworks.
# Under a single -syslibroot each is looked up in the SDK only. -Z drops
# them all.
root=$t/root
mkdir -p $root/usr/lib/swift $root/usr/local/lib \
  $root/Library/Frameworks/Fw.framework $root/System/Library/Frameworks/Fw.framework

dylib() {
  echo "int $2(void) { return $3; }" | $CC -shared -o $root$1 -xc - -install_name $1
}
dylib /usr/lib/swift/libfoo.dylib foo 1
dylib /usr/lib/libbar.dylib bar 2
dylib /usr/lib/swift/libbar.dylib bar 3
dylib /usr/local/lib/libbaz.dylib baz 4
dylib /usr/lib/swift/libqux.dylib qux 5
echo 'int qux(void) { return 6; }' | $CC -o $t/qux.o -c -xc -
ar rcs $root/usr/lib/libqux.a $t/qux.o
dylib /Library/Frameworks/Fw.framework/Fw fw 7
dylib /System/Library/Frameworks/Fw.framework/Fw fw 8

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo(void), bar(void), baz(void), qux(void), fw(void);
int main() { return foo() + bar() + baz() + qux() + fw(); }
EOF

link() {
  $mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 13.0 13.0} -syslibroot $root \
    $t/a.o $SDK/usr/lib/libSystem.tbd -lfoo -lbar -lbaz -lqux -framework Fw "$@"
}

link -o $t/exe
otool -L $t/exe > $t/libs
grep -q /usr/lib/swift/libfoo.dylib $t/libs
grep -q /usr/lib/libbar.dylib $t/libs
grep -q /usr/local/lib/libbaz.dylib $t/libs
not grep -q libqux $t/libs
grep -q /Library/Frameworks/Fw.framework/Fw $t/libs
not grep -q /System/Library/Frameworks/Fw.framework $t/libs

# A dylib anywhere on the path beats an archive with -search_dylibs_first.
link -o $t/exe2 -search_dylibs_first
otool -L $t/exe2 | grep -q /usr/lib/swift/libqux.dylib

not link -o $t/exe3 -Z 2> $t/log
grep -q "library 'foo' not found" $t/log
