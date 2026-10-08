#!/usr/bin/env bash
. $(dirname $0)/common.inc

# .gcc_except_table.f2 is folded into .gcc_except_table.f1 by ICF. Emitted
# relocations referring to the former via its section symbol must refer to
# the latter instead.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.section .text.f1,"ax",@progbits
.globl f1, lsda1
f1:
  .cfi_startproc
  .cfi_lsda 0x1b, .Llsda1
  mov $1, %eax
  ret
  .cfi_endproc
lsda1:
  lea .Llsda1(%rip), %rax
  ret

.section .text.f2,"ax",@progbits
.globl f2, lsda2
f2:
  .cfi_startproc
  .cfi_lsda 0x1b, .Llsda2
  mov $2, %eax
  ret
  .cfi_endproc
lsda2:
  lea .Llsda2(%rip), %rax
  ret

.section .gcc_except_table.f1,"a",@progbits
.Llsda1:
  .byte 0xff, 0xff, 0x01, 0x00

.section .gcc_except_table.f2,"a",@progbits
.Llsda2:
  .byte 0xff, 0xff, 0x01, 0x00
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int f1(), f2();
void *lsda1(), *lsda2();
int main() { printf("%d %d %d\n", f1(), f2(), lsda1() == lsda2()); }
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,--icf=all,--emit-relocs
$QEMU $t/exe | grep '^1 2 1$'

readelf -rW $t/exe > $t/log
[ $(grep -Ec 'R_X86_64_PC32 +[0-9a-f]+ \.gcc_except_table \+ 0$' $t/log) = 2 ]
[ $(grep -Ec 'R_X86_64_PC32 +[0-9a-f]+ \.gcc_except_table - 4$' $t/log) = 2 ]
