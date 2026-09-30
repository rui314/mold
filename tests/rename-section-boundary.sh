#!/bin/bash
source "$(dirname "$0")"/common.inc

# A section$start$ or section$end$ symbol names its section as an
# input section would be named - __DATA,__const is __DATA_CONST,__const
# of a final image, and -rename_section and -rename_segment apply - so
# it finds the section the image has rather than an empty one of the
# old name. A segment$start$ symbol follows -rename_segment.
cat <<'EOF' | $CC -o $t/a.o -c -xc -
#include <stdio.h>
extern char const_start __asm("section$start$__DATA$__const");
extern char const_end __asm("section$end$__DATA$__const");
extern char a_start __asm("section$start$__AAA$__a");
extern char b_start __asm("section$start$__BBB$__b");
extern char seg_start __asm("segment$start$__BBB");
void *const p = (void *)&p;
__attribute__((section("__AAA,__a"))) int a = 3;
int main() {
  printf("%p %p %p %p %p\n", &const_start, &const_end, &a_start, &b_start, &seg_start);
  printf("%p %p %p %p %p\n", &p, &p + 1, &a, &a, &a);
}
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { print $2 "," s; s = "" }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-rename_section,__AAA,__a,__BBB,__b \
  -Wl,-rename_segment,__BBB,__CCC
$t/exe > $t/out
[ "$(sed -n 1p $t/out)" = "$(sed -n 2p $t/out)" ]
sects $t/exe > $t/sects
grep -qx '__DATA_CONST,__const' $t/sects
grep -qx '__CCC,__b' $t/sects
not grep -q '__DATA,__const\|__AAA\|__BBB' $t/sects
