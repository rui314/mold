#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
void foo();
void bar() { foo(); }
EOF

./mold -shared -o $t/b.so $t/a.o -z pac-plt
readelf -n $t/b.so | grep 'AArch64 feature: PAC'
readelf --dynamic $t/b.so | grep AARCH64_PAC_PLT
readelf --dynamic $t/b.so | not grep AARCH64_BTI_PLT

# A PLT entry authenticates the address in x17 before jumping to it.
$OBJDUMP -d -j .plt $t/b.so | grep -A5 -E '<foo[@$]plt>:' > $t/log
grep -A1 -w autia1716 $t/log | grep 'br.*x17'
not grep -w bti $t/log

./mold -shared -o $t/b.so $t/a.o -z pac-plt -z force-bti
readelf -n $t/b.so | grep 'AArch64 feature: BTI, PAC'
readelf --dynamic $t/b.so | grep AARCH64_PAC_PLT
readelf --dynamic $t/b.so | grep AARCH64_BTI_PLT
$OBJDUMP -d -j .plt $t/b.so | grep -A5 -E '<foo[@$]plt>:' > $t/log
grep -A1 -w autia1716 $t/log | grep 'br.*x17'
