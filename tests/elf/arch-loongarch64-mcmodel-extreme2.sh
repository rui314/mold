#!/usr/bin/env bash
. $(dirname $0)/common.inc

# In the extreme code model, a PC-relative address is materialized by
# pcalau12i, addi.d, lu32i.d and lu52i.d, and all of them are relocated
# relative to the page of the pcalau12i. Here, the pcalau12i is at the end
# of a page and msg is exactly 2 GiB below that page, so lu52i.d's
# immediate would be 0xfff instead of 0 if computed from the next page.

cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.globl _start
_start:
  pcalau12i $t0, %pc_hi20(msg)
  addi.d    $t1, $zero, %pc_lo12(msg)
  lu32i.d   $t1, %pc64_lo20(msg)
  lu52i.d   $t1, $t1, %pc64_hi12(msg)
  add.d     $a1, $t0, $t1
  li.w      $a0, 1        # stdout
  li.w      $a2, 12       # strlen(msg)
  li.w      $a7, 64       # __NR_write
  syscall   0
  li.w      $a0, 0
  li.w      $a7, 93       # __NR_exit
  syscall   0

.data
msg:
  .ascii "Hello world\n"
EOF

./mold -o $t/exe $t/a.o --section-start=.text=0x100000ffc --section-start=.data=0x80000000
$QEMU $t/exe | grep -x 'Hello world'
