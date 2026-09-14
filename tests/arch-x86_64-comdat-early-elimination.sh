#!/usr/bin/env bash
. $(dirname $0)/common.inc

[ $MACHINE = x86_64 ] || skip

cat <<'EOF' | $CC -o $t/a.o -c -x assembler -
.section .text.foo,"axG",@progbits,foo,comdat
.globl foo
.type foo, @function
foo:
  .cfi_startproc
  ret
  .cfi_endproc
EOF

cat <<'EOF' | $CC -o $t/b.o -c -x assembler -
.section .text.foo,"axG",@progbits,foo,comdat
.globl foo
.type foo, @function
foo:
  .cfi_startproc
.Ldead:
  ret
  .cfi_endproc

.section .debug_addr,"",@progbits
  .quad .Ldead + 26

.section .debug_ranges,"",@progbits
  .quad .Ldead + 26
EOF

cat <<'EOF' | $CC -o $t/c.o -c -x assembler -
.text
.globl _start
_start:
  call foo
  ret
EOF

$CC -B. -nostdlib -Wl,-e,_start -o $t/exe $t/a.o $t/b.o $t/c.o

# The FDE and debug relocations in b.o refer to a discarded COMDAT member.
test "$(readelf -wf $t/exe | grep -c 'FDE cie=')" = 1
$OBJDUMP -s -j .debug_addr $t/exe | grep '0000 00000000 00000000'
$OBJDUMP -s -j .debug_ranges $t/exe | grep '0000 01000000 00000000'
