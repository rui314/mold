#!/usr/bin/env bash
. $(dirname $0)/common.inc

test_cflags -mbranch-protection=bti || skip

cat <<EOF | $CC -fPIC -mbranch-protection=none -c -o $t/a.o -xc -
void foo() {}
EOF

cat <<EOF | $CC -fPIC -mbranch-protection=bti -c -o $t/b.o -xc -
void foo();
void bar() { foo(); }
EOF

./mold -shared -o $t/c.so $t/a.o $t/b.o
readelf -n $t/c.so | not grep 'AArch64 feature'
$OBJDUMP -d -j .plt $t/c.so | grep -A1 '<_PROCEDURE_LINKAGE_TABLE_>:' | not grep -w bti

./mold -shared -o $t/c.so $t/a.o $t/b.o -z force-bti 2> $t/log
readelf -n $t/c.so | grep 'AArch64 feature: BTI'
readelf --dynamic $t/c.so | grep AARCH64_BTI_PLT
$OBJDUMP -d -j .plt $t/c.so | grep -A1 '<_PROCEDURE_LINKAGE_TABLE_>:' | grep -w bti
grep 'a.o: -z bti-report=warning: missing GNU_PROPERTY_AARCH64_FEATURE_1_BTI' $t/log
not grep b.o: $t/log

./mold -shared -o $t/c.so $t/a.o $t/b.o -z force-bti -z bti-report=none 2> $t/log
not grep a.o: $t/log

./mold -shared -o $t/c.so $t/a.o $t/b.o -z bti-report=warning 2> $t/log
readelf -n $t/c.so | not grep 'AArch64 feature'
grep 'a.o: -z bti-report=warning: missing GNU_PROPERTY_AARCH64_FEATURE_1_BTI' $t/log

not ./mold -shared -o $t/c.so $t/a.o $t/b.o -z bti-report=error 2> $t/log
grep 'a.o: -z bti-report=error: missing GNU_PROPERTY_AARCH64_FEATURE_1_BTI' $t/log
