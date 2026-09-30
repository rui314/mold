#!/usr/bin/env bash
. $(dirname $0)/common.inc

# a.o is marked with BTI and GCS, and b.o only with BTI.
cat <<EOF | $CC -c -o $t/a.o -xassembler-with-cpp -
#define NT_GNU_PROPERTY_TYPE_0 5
#define GNU_PROPERTY_AARCH64_FEATURE_1_AND 0xc0000000

  .globl _start
_start:
  ret

  .section .note.gnu.property, "a"
  .p2align 3
  .long 4
  .long 1f - 0f
  .long NT_GNU_PROPERTY_TYPE_0
  .asciz "GNU"
0:.long GNU_PROPERTY_AARCH64_FEATURE_1_AND
  .long 4
  .long 5
  .p2align 3
1:
EOF

cat <<EOF | $CC -c -o $t/b.o -xassembler-with-cpp -
#define NT_GNU_PROPERTY_TYPE_0 5
#define GNU_PROPERTY_AARCH64_FEATURE_1_AND 0xc0000000

  .globl foo
foo:
  ret

  .section .note.gnu.property, "a"
  .p2align 3
  .long 4
  .long 1f - 0f
  .long NT_GNU_PROPERTY_TYPE_0
  .asciz "GNU"
0:.long GNU_PROPERTY_AARCH64_FEATURE_1_AND
  .long 4
  .long 1
  .p2align 3
1:
EOF

# Older readelf doesn't know the name of the GCS bit.
./mold -o $t/exe1 $t/a.o
readelf -n $t/exe1 | grep -E 'AArch64 feature: BTI, (GCS|<unknown: 4>)$'

./mold -o $t/exe2 $t/a.o -z gcs=never
readelf -n $t/exe2 | grep 'AArch64 feature: BTI$'

./mold -o $t/exe3 $t/a.o $t/b.o 2> $t/log
readelf -n $t/exe3 | grep 'AArch64 feature: BTI$'
not grep . $t/log

./mold -o $t/exe4 $t/a.o $t/b.o -z gcs=always 2> $t/log
readelf -n $t/exe4 | grep -E 'AArch64 feature: BTI, (GCS|<unknown: 4>)$'
grep 'b.o: -z gcs-report=warning: missing GNU_PROPERTY_AARCH64_FEATURE_1_GCS' $t/log
not grep a.o: $t/log

./mold -o $t/exe5 $t/a.o $t/b.o -z gcs=always -z gcs-report=none 2> $t/log
not grep . $t/log

not ./mold -o $t/exe6 $t/a.o $t/b.o -z gcs-report=error 2> $t/log
grep 'b.o: -z gcs-report=error: missing GNU_PROPERTY_AARCH64_FEATURE_1_GCS' $t/log
