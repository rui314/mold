#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime lays out a final image's __thread_bss by atom size, smallest
# first and in input order among equals, whatever the command line or
# -order_file says; -r keeps the input order. An atom's size runs to
# the next atom of its object, padding included, so b.o's c and a.o's
# y count as 2 and 4 bytes, and the section comes to 0x18 bytes.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__thread int y;
__thread char d;
extern __thread long w;
extern __thread char c;
extern __thread short s;
int main() { printf("%d %d %ld %d %d\n", y, d, w, c, s); }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
__thread long w;
__thread char c;
__thread short s;
EOF

order() {
  nm -n $1 | sed -n 's/.* \(_.*\)\$tlv\$init$/\1/p' | tr '\n' ' '
}

$CC --ld-path=$mold -o $t/exe1 $t/b.o $t/a.o
$t/exe1 | grep -q '^0 0 0 0 0$'
order $t/exe1 | grep -q '^_d _c _s _y _w $'
otool -l $t/exe1 | grep -A3 'sectname __thread_bss' | grep -q 'size 0x0*18$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o
order $t/exe2 | grep -q '^_d _c _s _y _w $'

printf '_w$tlv$init\n_s$tlv$init\n' > $t/order
$CC --ld-path=$mold -o $t/exe3 $t/b.o $t/a.o -Wl,-order_file,$t/order
order $t/exe3 | grep -q '^_d _c _s _y _w $'

$CC --ld-path=$mold -o $t/c.o -r $t/b.o $t/a.o
order $t/c.o | grep -q '^_w _c _s _y _d $'
