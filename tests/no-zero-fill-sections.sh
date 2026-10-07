#!/bin/bash
source "$(dirname "$0")"/common.inc

# -no_zero_fill_sections gives zero-fill sections their bytes in the
# file, for a loader that copies segments without zero-filling them
# (XNU's x86-64 kernel links with it): they become regular sections,
# still last in their segment. An object file keeps them zero-fill.
cat <<EOF | $CC -o $t/a.o -c -xc - -fcommon
#include <stdio.h>
int data = 5;
static int bss[0x1000];
int common_sym[0x100];
int main() { bss[3] = 1; printf("%d %d %d\n", data, bss[3], common_sym[2]); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-no_zero_fill_sections
otool -l $t/exe > $t/lc
sect() { grep -A10 "sectname $2\$" $1 | awk '$1 == "offset" || $1 == "flags" { printf "%s %s ", $1, $2 }'; }
sect $t/lc __bss | grep -q ' flags 0x00000000 '
not grep -q 'offset 0 ' <(sect $t/lc __bss)
sect $t/lc __common | grep -q ' flags 0x00000000 '
not grep -q 'offset 0 ' <(sect $t/lc __common)
grep -A4 'segname __DATA$' $t/lc | awk '$1 == "vmsize" { v = $2 } $1 == "filesize" { f = $2 } END { exit !(v == sprintf("0x%016x", f)) }'
[ "$($RUN $t/exe)" = '5 1 0' ]

$mold -r -arch $ARCH -o $t/r.o $t/a.o -no_zero_fill_sections
otool -l $t/r.o > $t/lcr
sect $t/lcr __bss | grep -q 'offset 0 flags 0x00000001 '

# A thread-local zero-fill section must stay thread-local, as
# S_THREAD_LOCAL_REGULAR: dyld builds a thread's variables from the
# sections of the two thread-local types. (ld-prime makes it S_REGULAR,
# and each new thread then starts from the wrong bytes.)
cat <<EOF | $CC -o $t/b.o -c -xc -
#include <pthread.h>
#include <stdio.h>
__thread int tz[100];
__thread int td = 3;
void *thr(void *p) { tz[5] += 7; td += 1; printf("%d %d ", tz[5], td); return 0; }
int main() {
  tz[5] = 1;
  pthread_t t;
  pthread_create(&t, 0, thr, 0);
  pthread_join(t, 0);
  printf("%d %d\n", tz[5], td);
}
EOF
if $mold -v 2>&1 | grep -q mold-macho; then
  $CC --ld-path=$mold -o $t/exe2 $t/b.o -Wl,-no_zero_fill_sections
  otool -l $t/exe2 > $t/lc2
  sect $t/lc2 __thread_bss | grep -q ' flags 0x00000011 '
  [ "$($RUN $t/exe2)" = '7 4 1 3' ]
fi
