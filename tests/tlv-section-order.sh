#!/bin/bash
source "$(dirname "$0")"/common.inc

# dyld copies one template for each thread: the thread-local sections'
# initial values and zero fill, from the first such section to the
# last. ld-prime keeps the template contiguous and aligned by the
# section types, whatever the names: the initial values last among the
# file-backed __DATA sections, after the variables' descriptors, the
# zero fill first among the zero-fill ones, and all of its sections at
# the strictest alignment among them.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__mytdata,thread_local_regular
.p2align 5
_v\$tlv\$init:
  .quad 42
.section __DATA,__thread_vars,thread_local_variables
.globl _v
.p2align 3
_v:
  .quad __tlv_bootstrap
  .quad 0
  .quad _v\$tlv\$init
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
extern __thread long v;
__thread long x = 5;
__thread char y;
int main() { printf("%ld %ld %d\n", v, x, y); }
EOF

sections() {
  otool -l $1 | awk '/sectname/ {s = $2} /segname __DATA$/ {print s}' | tr '\n' ' '
}

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep '^42 5 0$'
sections $t/exe | grep -q '__thread_vars __mytdata __thread_data __thread_bss $'
otool -l $t/exe | grep -A5 'sectname __thread_data' | grep -q 'align 2^5'
otool -l $t/exe | grep -A5 'sectname __thread_bss' | grep -q 'align 2^5'

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
otool -l $t/r.o | grep -A5 'sectname __thread_data' | grep -q 'align 2^5'
otool -l $t/r.o | grep -A5 'sectname __thread_bss' | grep -q 'align 2^5'
