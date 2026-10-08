#!/bin/bash
source "$(dirname "$0")"/common.inc

# -simulator_support marks a macOS dylib that dyld may load into an iOS
# simulator process too: MH_SIM_SUPPORT in its header. ld-prime sets the
# flag on a dylib alone.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 3; }
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -Wl,-simulator_support
otool -hv $t/b.dylib | grep -q SIM_SUPPORT

$CC --ld-path=$mold -o $t/c.dylib -shared $t/a.o
otool -hv $t/c.dylib > $t/log
not grep -q SIM_SUPPORT $t/log

for kind in '' -bundle; do
  $CC --ld-path=$mold -o $t/exe $kind $t/a.o -Wl,-simulator_support
  otool -hv $t/exe > $t/log
  not grep -q SIM_SUPPORT $t/log
done
