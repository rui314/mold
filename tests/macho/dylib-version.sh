#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int foo() { return 1; }
EOF2

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o \
  -Wl,-current_version,2.3.4 -Wl,-compatibility_version,2.0.0
otool -l $t/libfoo.dylib | grep -A5 LC_ID_DYLIB > $t/log
grep -q 'current version 2.3.4' $t/log
grep -q 'compatibility version 2.0.0' $t/log

# -final_output names the dylib when -install_name is absent (the
# compiler driver passes it with several -arch; Transmission's CMake
# build does too).
$CC --ld-path=$mold -shared -o $t/libbaz.dylib $t/a.o -Wl,-final_output,/opt/lib/libbaz.dylib
otool -D $t/libbaz.dylib | grep -q '^/opt/lib/libbaz.dylib$'
$CC --ld-path=$mold -shared -o $t/libqux.dylib $t/a.o -Wl,-final_output,/opt/lib/libqux.dylib \
  -Wl,-install_name,/explicit/libqux.dylib
otool -D $t/libqux.dylib | grep -q '^/explicit/libqux.dylib$'

# The older -dylib_ spellings, which Xcode still passes.
$CC --ld-path=$mold -shared -o $t/libbar.dylib $t/a.o \
  -Wl,-dylib_current_version,5.6.7 -Wl,-dylib_compatibility_version,5.0.0
otool -l $t/libbar.dylib | grep -A5 LC_ID_DYLIB > $t/log
grep -q 'current version 5.6.7' $t/log
grep -q 'compatibility version 5.0.0' $t/log
