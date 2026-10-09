#!/bin/bash
source "$(dirname "$0")"/common.inc

# __thread_bss lays its subsections out like any other section's: in
# input order, with -order_file's first. (ld-prime lays out a final
# image's by subsection size, whatever -order_file says.) Each variable
# keeps a place of its own, zero at first.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
__thread int y;
__thread char d;
extern __thread long w;
extern __thread char c;
extern __thread short s;
int main() {
  printf("%d %d %ld %d %d\n", y, d, w, c, s);
  y = 1; d = 2; w = 3; c = 4; s = 5;
  printf("%d %d %ld %d %d\n", y, d, w, c, s);
}
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
$RUN $t/exe1 > $t/out1
diff - $t/out1 <<EOF
0 0 0 0 0
1 2 3 4 5
EOF
order $t/exe1 | grep -q '^_w _c _s _y _d $'

$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o
order $t/exe2 | grep -q '^_y _d _w _c _s $'

printf '_s$tlv$init\n_y$tlv$init\n' > $t/order
$CC --ld-path=$mold -o $t/exe3 $t/b.o $t/a.o -Wl,-order_file,$t/order
order $t/exe3 | grep -q '^_s _y _w _c _d $'
$RUN $t/exe3 | grep '^1 2 3 4 5$'

$CC --ld-path=$mold -o $t/c.o -r $t/b.o $t/a.o
order $t/c.o | grep -q '^_w _c _s _y _d $'
