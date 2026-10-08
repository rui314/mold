#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Deduplication folds identical functions only if they are unwound
# alike, as mold's ICF hashes a section's CIE and FDEs: the survivor's
# unwind information describes the frames of every caller of the copies.
# Copies with the same compact encoding fold, and so do copies with the
# same DWARF CFI; a copy with unwind information and one without, copies
# with different encodings and copies with different FDEs stay apart.
# (ld-prime compares only the personality and the LSDA, so it folds them
# all, and an exception through a caller of a folded copy is unwound by
# the survivor's rules.)
if [ $ARCH = arm64 ]; then
  prologue() { echo 'stp x29, x30, [sp, #-16]!'; echo 'mov x29, sp'; }
  body() { echo 'mov w0, #5'; echo 'ldp x29, x30, [sp], #16'; echo ret; }
  frame() {
    echo '.cfi_def_cfa w29, 16'
    echo '.cfi_offset w30, -8'
    echo '.cfi_offset w29, -16'
  }
  frameless() { echo '.cfi_def_cfa_offset 16'; }
  call() { echo "bl $1"; }
else
  prologue() { echo 'pushq %rbp'; echo 'movq %rsp, %rbp'; }
  body() { echo 'movl $5, %eax'; echo 'popq %rbp'; echo ret; }
  frame() {
    echo '.cfi_def_cfa_offset 16'
    echo '.cfi_offset %rbp, -16'
    echo '.cfi_def_cfa_register %rbp'
  }
  frameless() { echo '.cfi_def_cfa_offset 16'; }
  call() { echo "call $1"; }
fi

# func <name> <cfi>...: a private extern function of the same code,
# with the given CFI after its prologue, or with none.
func() {
  local name=$1
  shift
  echo ".globl $name"
  echo ".private_extern $name"
  echo '.p2align 2'
  echo "$name:"
  [ $# = 0 ] || echo '.cfi_startproc'
  prologue
  for cfi in "$@"; do $cfi; done
  body
  [ $# = 0 ] || echo '.cfi_endproc'
}
# An escape the compact encoding can't express makes an FDE.
args16() { echo '.cfi_escape 0x2e, 0x10'; }
args32() { echo '.cfi_escape 0x2e, 0x20'; }

{
  echo '.subsections_via_symbols'
  echo '.text'
  func _none
  func _frame1 frame
  func _frame2 frame
  func _frameless frameless
  func _dwarf1 frame args16
  func _dwarf2 frame args16
  func _dwarf3 frame args32
  echo '.globl _call_all'
  echo '.p2align 2'
  echo '_call_all:'
  echo '.cfi_startproc'
  prologue
  frame
  for f in _none _frame1 _frame2 _frameless _dwarf1 _dwarf2 _dwarf3; do
    call $f
  done
  body
  echo '.cfi_endproc'
} > $t/a.s
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/b.o -c -xc -
int call_all(void);
int main() { return call_all() != 5; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o -Wl,-deduplicate
$RUN $t/exe
nm $t/exe > $t/nm

# Two symbols name one address
same_addr() {
  a=$(grep " $1\$" $t/nm | cut -d' ' -f1)
  b=$(grep " $2\$" $t/nm | cut -d' ' -f1)
  [ -n "$a" ] && [ "$a" = "$b" ]
}

same_addr _frame1 _frame2
same_addr _dwarf1 _dwarf2

if $mold -v 2>&1 | grep -q mold-macho; then
  not same_addr _none _frame1
  not same_addr _frame1 _frameless
  not same_addr _frame1 _dwarf1
  not same_addr _dwarf1 _dwarf3

  # Each function keeps its own unwind information: none, its compact
  # encoding, or DWARF mode pointing at its own FDE.
  unwind_lookup $t/exe _none _frame1 _frameless _dwarf1 _dwarf3 > $t/enc
  enc() { sed -n ${1}p $t/enc; }
  [ "$(enc 1)" = 0x0 ]
  [ "$(enc 2)" != 0x0 ]
  [ "$(enc 3)" != 0x0 ]
  [ "$(enc 2)" != "$(enc 3)" ]
  [ "$(enc 4)" != "$(enc 5)" ]
  dwarfdump --eh-frame $t/exe > $t/eh
  [ "$(grep -c ' FDE ' $t/eh)" = 2 ]
fi
