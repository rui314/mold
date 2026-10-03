#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime names an output section in steps: a __DATA section that
# needs no writes after fixups moves to __DATA_CONST, the first
# -rename_section naming that name renames it, and the first
# -rename_segment naming the resulting segment moves it on - after a
# -rename_section too. A literal pool still in __TEXT then merges into
# __const (under that section's new name); a renamed one keeps its
# name and type. A new name is cut to 16 bytes.
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
const char *const msg = "hello";
__attribute__((section("__AAA,__a"))) int a = 3;
__attribute__((constructor)) static void init(void) { a++; }
int main() { puts(msg); return a != 4; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.p2align 3
.quad 12345
EOF

cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _main
_main:
  ret
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { print $2 "," s; s = "" }'
}

# Without renames, for reference.
$CC --ld-path=$mold -o $t/exe0 $t/a.o
sects $t/exe0 > $t/sects0
grep -qx '__DATA_CONST,__const' $t/sects0
grep -qx '__AAA,__a' $t/sects0
$t/exe0 | grep -q hello

# -rename_section matches the __DATA_CONST name, not the input's.
$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-rename_section,__DATA,__const,__FOO,__bar
sects $t/exe1 > $t/sects1
grep -qx '__DATA_CONST,__const' $t/sects1
not grep -q '__FOO' $t/sects1

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-rename_section,__DATA_CONST,__const,__FOO,__bar
sects $t/exe2 > $t/sects2
grep -qx '__FOO,__bar' $t/sects2
not grep -q '__DATA_CONST,__const' $t/sects2
$t/exe2 | grep -q hello

# -rename_segment applies after a -rename_section.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-rename_section,__AAA,__a,__BBB,__b \
  -Wl,-rename_segment,__BBB,__CCC
sects $t/exe3 > $t/sects3
grep -qx '__CCC,__b' $t/sects3
not grep -q '__AAA\|__BBB' $t/sects3
$t/exe3 | grep -q hello

# A literal pool merges into __TEXT,__const unless it is renamed.
$CC --ld-path=$mold -o $t/exe4 $t/a.o $t/b.o
grep -qx '__TEXT,__const' <(sects $t/exe4)
not grep -q '__literal8' <(sects $t/exe4)
$CC --ld-path=$mold -o $t/exe5 $t/a.o $t/b.o -Wl,-rename_section,__TEXT,__literal8,__FOO,__l8
otool -l $t/exe5 | grep -A9 'sectname __l8' | grep -q 'flags 0x00000004'
$CC --ld-path=$mold -o $t/exe6 $t/a.o $t/b.o -Wl,-rename_section,__TEXT,__const,__FOO,__c
grep -qx '__FOO,__c' <(sects $t/exe6)
not grep -q '__TEXT,__const\|__literal8' <(sects $t/exe6)

# New names longer than 16 bytes are cut; missing ones are errors.
$CC --ld-path=$mold -o $t/exe7 $t/a.o \
  -Wl,-rename_section,__AAA,__a,__BBBBBBBBBBBBBBBBBBBB,__bbbbbbbbbbbbbbbbbbbb
grep -qx '__BBBBBBBBBBBBBB,__bbbbbbbbbbbbbb' <(sects $t/exe7)

not $mold -arch $ARCH -static -e _main -o $t/exe8 $t/c.o -rename_segment __AAA '' 2> $t/log8
grep -q -- '-rename_segment.*missing' $t/log8
not $mold -arch $ARCH -static -e _main -o $t/exe8 $t/c.o -rename_section __AAA __a __B 2> $t/log8
grep -q -- '-rename_section.*missing' $t/log8
