#!/bin/bash
source "$(dirname "$0")"/common.inc

# The libraries the command line names get their load commands (and
# ordinals) in naming order, and the ones auto-link options name and
# those loaded as another's re-export get theirs too, the same whatever
# order the options come in. (ld-prime lists the auto-linked and the
# re-exported ones last, together, by install name.)
echo 'int foo(void) { return 3; }' | $CC -o $t/foo.o -c -xc -
echo 'int bar(void) { return 4; }' | $CC -o $t/bar.o -c -xc -
$CC -o $t/libqfoo.dylib -shared $t/foo.o -Wl,-install_name,/zzz/libqfoo.dylib
$CC -o $t/libqbar.dylib -shared $t/bar.o -Wl,-install_name,/AAA/libqbar.dylib

cat <<EOF | $CC -o $t/a.o -c -xc -
__asm__(".linker_option \"-lqfoo\"");
__asm__(".linker_option \"-lqbar\"");
typedef const void *CFTypeRef;
void CFRelease(CFTypeRef);
int foo(void), bar(void);
int main() { CFRelease(0); return foo() + bar(); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -L$t -framework Foundation
otool -L $t/exe | tail -n +2 | awk '{print $1}' | sed 's|.*/||' > $t/libs
[ "$(sort $t/libs | tr '\n' ' ')" = "CoreFoundation Foundation libSystem.B.dylib libqbar.dylib libqfoo.dylib " ]
[ "$(grep -e '^Foundation$' -e '^libSystem' $t/libs | tr '\n' ' ')" = "Foundation libSystem.B.dylib " ]

cat <<EOF | $CC -o $t/b.o -c -xc -
__asm__(".linker_option \"-lqbar\"");
__asm__(".linker_option \"-lqfoo\"");
typedef const void *CFTypeRef;
void CFRelease(CFTypeRef);
int foo(void), bar(void);
int main() { CFRelease(0); return foo() + bar(); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/b.o -L$t -framework Foundation
otool -L $t/exe2 | tail -n +2 | awk '{print $1}' | sed 's|.*/||' | diff $t/libs -
