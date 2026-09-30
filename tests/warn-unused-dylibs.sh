#!/bin/bash
source "$(dirname "$0")"/common.inc

# A dylib bound for the dyld shared cache - installed in /usr/lib,
# /System/Library or those under /Library/Apple, and neither
# -not_for_dyld_shared_cache nor -debug_variant - gets ld-prime's
# warning for each library it links but binds nothing from;
# -warn_unused_dylibs asks for it for any output. -needed_* and -reexport_* libraries are linked on purpose,
# and libSystem, libc++ and Foundation are let off.
echo 'int x(void) { return 1; }' | $CC -o $t/a.o -c -xc -

link() { $CC --ld-path=$mold -shared -o $t/b.dylib $t/a.o "$@" 2> $t/log; }
msg="linking with (/usr/lib/libz.1.dylib) but not using any symbols from it"

link -Wl,-install_name,/usr/lib/libb.dylib -lz
grep -qF "$msg" $t/log

link -Wl,-install_name,/System/Library/PrivateFrameworks/B.framework/B -lz -lc++ \
  -framework Foundation -framework CoreFoundation
grep -qF "$msg" $t/log
grep -q 'linking with (/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation)' $t/log
not grep -q 'libc++.1.dylib' $t/log
not grep -q 'Foundation.framework/Versions/C/Foundation' $t/log

link -Wl,-install_name,/usr/lib/libb.dylib -Wl,-needed-lz
not grep -q 'not using any symbols' $t/log
link -Wl,-install_name,/usr/local/lib/libb.dylib -lz
not grep -q 'not using any symbols' $t/log
link -Wl,-install_name,/usr/lib/libb.dylib -lz -Wl,-no_warn_unused_dylibs
not grep -q 'not using any symbols' $t/log
link -Wl,-install_name,/usr/lib/libb.dylib -lz -Wl,-not_for_dyld_shared_cache
not grep -q 'not using any symbols' $t/log
link -Wl,-install_name,/usr/lib/libb.dylib -lz -Wl,-debug_variant
not grep -q 'not using any symbols' $t/log
link -Wl,-install_name,/Library/Apple/usr/lib/libb.dylib -lz
grep -qF "$msg" $t/log

echo 'int main() { return 0; }' | $CC -o $t/m.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/m.o -lz 2> $t/log
not grep -q 'not using any symbols' $t/log
$CC --ld-path=$mold -o $t/exe $t/m.o -lz -Wl,-warn_unused_dylibs 2> $t/log
grep -qF "$msg" $t/log
