#!/bin/bash
source "$(dirname "$0")"/common.inc

# Where a relocation refers to its target by section and address - a
# __compact_unwind record's function or LSDA, or an x86-64
# section-relative relocation - a -r output keeps it so, the address
# moved to the merged layout. A later link finds the subsection by the
# address, as this one did. (ld-prime names a symbol at the place
# instead.)
for subs in '' .subsections_via_symbols; do
  if [ $ARCH = arm64 ]; then
    cat <<EOF | $CC -o $t/a.o -c -xassembler -
$subs
.globl _main, _w, _g
.weak_definition _w
.text
.p2align 2
_main:
  .cfi_startproc
  ret
  .cfi_endproc
L_in_main:
  .cfi_startproc
  ret
  .cfi_endproc
_aa:
_zz:
  .cfi_startproc
  ret
  .cfi_endproc
_w:
_l:
  .cfi_startproc
  ret
  .cfi_endproc
_k:
_g:
  .cfi_startproc
  ret
  .cfi_endproc
L_in_g:
  .cfi_startproc
  ret
  .cfi_endproc
EOF

    $mold -arch $ARCH -r $t/a.o -o $t/r.o
    otool -rv $t/r.o > $t/relocs
    sed -n '/__compact_unwind/,$p' $t/relocs | awk 'NR > 2 {print $1, $5, $NF}' > $t/names
    [ "$(grep -c ' False (__TEXT,__text)$' $t/names)" = 6 ]
    # The images linked from the object and from the output unwind
    # the same functions alike.
    $CC --ld-path=$mold -o $t/exe1 $t/a.o
    $CC --ld-path=$mold -o $t/exe2 $t/r.o
    objdump --macho --unwind-info $t/exe1 | tail -n +2 > $t/unwind1
    objdump --macho --unwind-info $t/exe2 | tail -n +2 > $t/unwind2
    diff $t/unwind1 $t/unwind2
    continue
  fi

  # x86-64 section-relative relocations, to a named place or not.
  cat <<EOF | $CC -o $t/b.o -c -xassembler -
$subs
.globl _w, _p, _ptrs
.private_extern _p
.weak_definition _w
.text
L_start:
  nop
_w:
_p:
_l:
  nop
  nop
  ret
.data
.p2align 3
_ptrs:
  .quad L_start
  .quad L_start + 1
  .quad L_start + 2
EOF

  $mold -arch $ARCH -r $t/b.o -o $t/r.o
  otool -rv $t/r.o | awk 'NR > 2 {print $1, $5, $NF}' > $t/names
  grep -qx '00000000 False (__TEXT,__text)' $t/names
  grep -qx '00000008 False (__TEXT,__text)' $t/names
  grep -qx '00000010 False (__TEXT,__text)' $t/names

  # The fields still point where they did.
  cat <<EOF | $CC -o $t/c.o -c -xc -
extern char w[];
extern char *ptrs[];
int main() { return !(ptrs[0] == w - 1 && ptrs[1] == w && ptrs[2] == w + 1); }
EOF
  $CC --ld-path=$mold -o $t/exe $t/r.o $t/c.o
  $t/exe
done
