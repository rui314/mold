#!/bin/bash
source "$(dirname "$0")"/common.inc

# A segment$start$ or segment$end$ symbol naming a segment the image
# lacks - no input has one, or -rename_section emptied it - gets an
# empty segment to point at, as ld-prime makes it: no sections, vmsize
# 0, just before __LINKEDIT and at its address.
cat <<'EOF' | $CC -o $t/a.o -c -xc -
#include <stdio.h>
extern char zzz_start __asm("segment$start$__ZZZ");
extern char zzz_end __asm("segment$end$__ZZZ");
extern char aaa_start __asm("segment$start$__AAA");
__attribute__((section("__AAA,__a"))) int a = 3;
int main() { printf("%p %p %p %d\n", &zzz_start, &zzz_end, &aaa_start, a); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-rename_section,__AAA,__a,__DATA,__a
read -r s e a v <<< "$($RUN $t/exe)"
[ $s = $e ] && [ $s = $a ] && [ $v = 3 ]

otool -l $t/exe > $t/lc
segs=$(awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }' $t/lc)
[ "${segs#*__DATA }" = '__ZZZ __AAA __LINKEDIT ' ]
seg() { awk -v s=$1 -v f=$2 '$1 == "segname" { n = $2 } n == s && $1 == f { print $2; exit }' $t/lc; }
[ $(seg __ZZZ vmsize) = 0x0000000000000000 ]
[ $(seg __ZZZ nsects) = 0 ]
[ $(seg __ZZZ vmaddr) = $(seg __LINKEDIT vmaddr) ]
[ $(seg __AAA vmaddr) = $(seg __LINKEDIT vmaddr) ]
