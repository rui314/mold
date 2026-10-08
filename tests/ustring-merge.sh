#!/bin/bash
source "$(dirname "$0")"/common.inc

# __TEXT,__ustring holds the UTF-16 strings of CFString constants that
# aren't ASCII, and C's u"" literals. ld-prime cuts it into subsections
# at its symbols and merges each with identical ones, whatever labels
# it; a CFString of each object that spells the same string then merges
# too.
for n in 1 2; do
  cat <<EOF | $CC -o $t/a$n.o -c -xc -
#include <CoreFoundation/CoreFoundation.h>
CFStringRef cf$n(void) { return CFSTR("héllo wörld"); }
const void *u$n(void) { return u"plain utf16"; }
EOF
done
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
CFStringRef cf1(void), cf2(void);
const void *u1(void), *u2(void);
int main() {
  printf("%d %d %ld\n", cf1() == cf2(), u1() == u2(), (long)CFStringGetLength(cf1()));
}
EOF

size() {
  otool -l $1 | awk -v s=$2 '$1 == "sectname" && $2 == s { f = 1 }
    f && $1 == "size" { print $2; f = 0 }'
}

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a1.o $t/a2.o -framework CoreFoundation
$RUN $t/exe | grep -q '^1 1 11$'
[ "$(size $t/exe __ustring)" = 0x0000000000000030 ]
[ "$(size $t/exe __cfstring)" = 0x0000000000000020 ]

# A -r output leaves them to the final link, which merges them as it
# does the objects'.
$mold -r -arch $ARCH -o $t/r.o $t/a1.o $t/a2.o
[ "$(size $t/r.o __ustring)" = 0x0000000000000060 ]
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/r.o -framework CoreFoundation
$RUN $t/exe3 | grep -q '^1 1 11$'
[ "$(size $t/exe3 __ustring)" = 0x0000000000000030 ]
[ "$(size $t/exe3 __cfstring)" = 0x0000000000000020 ]

# So does a subsection a symbol other than an l-label names, or one of
# several strings.
for n in 1 2; do
  cat <<EOF | $CC -o $t/b$n.o -c -xassembler -
.section __TEXT,__ustring
.p2align 1
_n$n: .short 0x61, 0x62, 0
l_m$n: .short 0x61, 0x62, 0x63, 0, 0x64, 0
.data
.p2align 3
.globl _p$n
_p$n: .quad _n$n, l_m$n
.subsections_via_symbols
EOF
done
cat <<EOF | $CC -o $t/main2.o -c -xc -
#include <stdio.h>
extern const void *p1[2], *p2[2];
int main() { printf("%d %d\n", p1[0] == p2[0], p1[1] == p2[1]); }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/main2.o $t/b1.o $t/b2.o
$RUN $t/exe2 | grep -q '^1 1$'
[ "$(size $t/exe2 __ustring)" = 0x0000000000000012 ]
