#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A GOT-relative relocation in a position-independent output can't refer
# to a preemptible symbol or to a symbol at a fixed address. The error
# message should tell which is the case.

case $MACHINE in
x86_64)
  cat <<EOF > $t/a.s
  .globl foo, get_foo
  .data
foo:
  .long 42
  .text
get_foo:
  movabs \$foo@GOTOFF, %rax
  ret
EOF

  cat <<EOF > $t/b.s
  .globl main
  .weak bar
  .hidden bar
main:
  movabs \$bar@GOTOFF, %rax
  movabs \$baz@GOTOFF, %rax
  xor %eax, %eax
  ret
EOF
  ;;
i686)
  cat <<EOF > $t/a.s
  .globl foo, get_foo
  .data
foo:
  .long 42
  .text
get_foo:
  call 1f
1:
  pop %ecx
  addl \$_GLOBAL_OFFSET_TABLE_+[.-1b], %ecx
  mov foo@GOTOFF(%ecx), %eax
  ret
EOF

  cat <<EOF > $t/b.s
  .globl main
  .weak bar
  .hidden bar
main:
  call 1f
1:
  pop %ecx
  addl \$_GLOBAL_OFFSET_TABLE_+[.-1b], %ecx
  lea bar@GOTOFF(%ecx), %eax
  lea baz@GOTOFF(%ecx), %eax
  xor %eax, %eax
  ret
EOF
  ;;
*)
  skip
  ;;
esac

$CC -c -o $t/a.o $t/a.s
$CC -c -o $t/b.o $t/b.s

not $CC -B. -shared -o $t/c.so $t/a.o |&
  grep 'against preemptible symbol .foo. can not be used; recompile with -fPIC, or make the symbol non-preemptible'

$CC -B. -shared -o $t/c.so $t/a.o -Wl,-Bsymbolic

not $CC -B. -pie -o $t/exe $t/b.o -Wl,-defsym,baz=0x1000 >& $t/log
grep 'against undefined symbol .bar. can not be used when making a position-independent output' $t/log
grep 'against absolute symbol .baz. can not be used when making a position-independent output' $t/log
