#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime merges identical fixed-size literals only in the standard
# pools, __TEXT,__literal4/8/16 of their own types; records of a
# literal type in a section of another name (or a pool of another
# type) all stay. A pool of either type then joins __TEXT,__const.
# A record with a relocation never merges: its bytes are the addend
# alone, so identical records may point at different targets.
for n in a b; do
  if [ $ARCH = arm64 ]; then
    load="adrp x8, Lp_$n@PAGE
  ldr x8, [x8, Lp_$n@PAGEOFF]
  ldr w0, [x8]
  ret"
  else
    load="movq Lp_$n(%rip), %rax
  movl (%rax), %eax
  ret"
  fi
  cat <<EOF | $CC -o $t/$n.o -c -xassembler -
.section __DATA,__lit,8byte_literals
Lp_$n: .quad _x_$n
.section __DATA,__lit2,8byte_literals
.quad 42
.quad 42
.literal8
.quad 43
.quad 43
.section __TEXT,__literal4,4byte_literals
.long 44
.long 44
.data
.globl _x_$n
_x_$n: .long $([ $n = a ] && echo 1 || echo 2)
.text
.globl _get_$n
.p2align 2
_get_$n:
  $load
EOF
done

cat <<EOF | $CC -o $t/c.o -c -xc -
#include <stdio.h>
int get_a(void);
int get_b(void);
int main() { printf("%d %d\n", get_a(), get_b()); }
EOF

# Makes b.o's __literal4 an 8-byte literal pool.
set_section_flags $t/b.o __TEXT __literal4 4

sect() {
  otool -l $1 | awk -v g=$2 -v s=$3 '$1 == "sectname" { n = $2 }
    $1 == "segname" && n == s && $2 == g { f = 1 } f && $1 == "size" { print $2; f = 0 }'
}

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o $t/c.o
$RUN $t/exe | grep -q '^1 2$'
[ "$(sect $t/exe __DATA __lit)" = 0x0000000000000010 ]
[ "$(sect $t/exe __DATA __lit2)" = 0x0000000000000020 ]
[ "$(sect $t/exe __TEXT __literal4)" = '' ]
# __literal8's 43s merge into one in __TEXT,__const, and so do a.o's
# 44s in __literal4's; b.o's pool of 8-byte literals named __literal4
# joins them with its record.
[ "$(sect $t/exe __TEXT __const)" = 0x0000000000000018 ]

# A -r output leaves the merging to the final link, which merges as
# it does the objects'.
$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o
[ "$(sect $t/r.o __TEXT __literal8)" = 0x0000000000000020 ]
$CC --ld-path=$mold -o $t/exe2 $t/r.o $t/c.o
$RUN $t/exe2 | grep -q '^1 2$'
[ "$(sect $t/exe2 __DATA __lit)" = 0x0000000000000010 ]
[ "$(sect $t/exe2 __DATA __lit2)" = 0x0000000000000020 ]

# Nor do __literal8 records with relocations merge, which ld-prime
# does: its -r output points both at _x_a.
for n in a b; do
  cat <<EOF | $CC -o $t/d$n.o -c -xassembler -
.literal8
.quad _y_$n
.data
.globl _y_$n
_y_$n: .long 0
EOF
done
if $mold -v 2>&1 | grep -q mold-macho; then
  $mold -r -arch $ARCH -o $t/r2.o $t/da.o $t/db.o
  [ "$(sect $t/r2.o __TEXT __literal8)" = 0x0000000000000010 ]
  objdump --macho -r $t/r2.o > $t/relocs
  grep -q '_y_a$' $t/relocs
  grep -q '_y_b$' $t/relocs
fi
