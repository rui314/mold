#!/bin/bash
source "$(dirname "$0")"/common.inc

# Where a -r relocation is re-derived from an address - a
# __compact_unwind record's function or LSDA, or an x86-64
# section-relative relocation - ld-prime names the symbol there: of
# several at one place, the first by non-weak before weak, then
# global, private external and local, each by descending name, an
# arm64 ltmpN label last. Past the place, into the subsection's bytes,
# it names the first too, plus the offset, as the names are aliases of
# one subsection - except in an object without subsections away from a
# section's start, where each name is a subsection of its own, all
# empty but the last, which holds the bytes and is named.
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
    sed -n '/__compact_unwind/,$p' $t/relocs | awk 'NR > 2 {print $1, $NF}' > $t/names
    grep -qx '00000000 _main' $t/names
    grep -qx '00000020 _main' $t/names
    grep -qx '00000040 _zz' $t/names
    grep -qx '00000060 _l' $t/names
    grep -qx '00000080 _g' $t/names
    if [ -z "$subs" ]; then
      grep -qx '000000a0 _k' $t/names
    else
      grep -qx '000000a0 _g' $t/names
    fi
    # The function fields of the two past a place hold their offsets.
    otool -s __LD __compact_unwind $t/r.o > $t/unwind
    awk 'NR == 5 || NR == 13 {printf "%s ", $2}' $t/unwind | grep -qx '00000004 00000004 '
    continue
  fi

  # An x86-64 section-relative relocation to a named place becomes an
  # extern one; one to a nameless place stays section-relative.
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
  grep -qx '00000008 True _p' $t/names
  if [ -z "$subs" ]; then
    grep -qx '00000010 True _w' $t/names
  else
    grep -qx '00000010 True _p' $t/names
  fi

  # The fields still point where they did.
  cat <<EOF | $CC -o $t/c.o -c -xc -
extern char w[];
extern char *ptrs[];
int main() { return !(ptrs[0] == w - 1 && ptrs[1] == w && ptrs[2] == w + 1); }
EOF
  $CC --ld-path=$mold -o $t/exe $t/r.o $t/c.o
  $t/exe
done
