#!/bin/bash
. $(dirname $0)/common.inc

# Skip if target is not x86-64
echo ret | cc -c -o /dev/null -xassembler -Wa,-mrelax-relocations=yes - 2> /dev/null || skip

cat <<EOF | $CC -o $t/a.o -c -x assembler -Wa,-mrelax-relocations=yes - || skip
.globl bar
bar:
  mov foo@gotpcrel(%rip), %rax
  mov foo@gotpcrel(%rip), %rcx
  mov foo@gotpcrel(%rip), %rdx
  mov foo@gotpcrel(%rip), %rbx
  mov foo@gotpcrel(%rip), %rbp
  mov foo@gotpcrel(%rip), %rsi
  mov foo@gotpcrel(%rip), %rdi
  mov foo@gotpcrel(%rip), %r8
  mov foo@gotpcrel(%rip), %r9
  mov foo@gotpcrel(%rip), %r10
  mov foo@gotpcrel(%rip), %r11
  mov foo@gotpcrel(%rip), %r12
  mov foo@gotpcrel(%rip), %r13
  mov foo@gotpcrel(%rip), %r14
  mov foo@gotpcrel(%rip), %r15
  mov foo@gotpcrel(%rip), %r16
  mov foo@gotpcrel(%rip), %r17
  mov foo@gotpcrel(%rip), %r18
  mov foo@gotpcrel(%rip), %r19
  mov foo@gotpcrel(%rip), %r20
  mov foo@gotpcrel(%rip), %r21
  mov foo@gotpcrel(%rip), %r22
  mov foo@gotpcrel(%rip), %r23
  mov foo@gotpcrel(%rip), %r24
  mov foo@gotpcrel(%rip), %r25
  mov foo@gotpcrel(%rip), %r26
  mov foo@gotpcrel(%rip), %r27
  mov foo@gotpcrel(%rip), %r28
  mov foo@gotpcrel(%rip), %r29
  mov foo@gotpcrel(%rip), %r30
  mov foo@gotpcrel(%rip), %r31

  call *foo@gotpcrel(%rip)
  jmp  *foo@gotpcrel(%rip)
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void foo() {}
int main() {}
EOF

$CC -B. -o $t/exe $t/a.o $t/b.o
$OBJDUMP -d $t/exe | sed -n '/<bar>:/,/<.*>:/p' > $t/log

grep -E 'lea \s*0x.+\(%rip\),%rax .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%rcx .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%rdx .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%rbx .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%rbp .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%rsi .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%rdi .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r8  .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r9  .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r10 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r11 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r12 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r13 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r14 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r15 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r16 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r17 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r18 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r19 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r20 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r21 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r22 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r23 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r24 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r25 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r26 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r27 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r28 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r29 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r30 .*<foo>' $t/log
grep -E 'lea \s*0x.+\(%rip\),%r31 .*<foo>' $t/log

grep -E 'call.*<foo>' $t/log
grep -E 'jmp.*<foo>' $t/log
