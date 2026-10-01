#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime gives the libraries the command line names their load
# commands (and ordinals) in naming order, then the ones auto-link
# options name together with those loaded as another's re-export, by
# install name: an auto-linked /AAA/libqbar goes before CoreFoundation,
# which Foundation re-exports, and /zzz/libqfoo after it, whatever
# order the options come in.
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
otool -L $t/exe | tail -n +2 | awk '{print $1}' | sed 's|.*/||' | tr '\n' ' ' > $t/libs
[ "$(cat $t/libs)" = "Foundation libSystem.B.dylib libqbar.dylib CoreFoundation libqfoo.dylib " ]
