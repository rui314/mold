#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 carries the inputs' linker optimization hints through a -r link
# for the final link to apply. A hint moves with its subsection and is
# dropped with a coalesced-away weak copy; one that doesn't lie in a
# single subsection of code is dropped as the object is read. The
# output lists the hints subsection by subsection in address order,
# each subsection's in input order. ld-prime 27037 drops every hint in
# -r, so it fails this test; ld-classic passes it.
[ $ARCH = arm64 ] || skip

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.p2align 2
.globl _cold, _f1, _f2, _wk, _span1, _span2
.weak_definition _wk
_cold:
L1: adrp x0, _v@PAGE
L2: add x0, x0, _v@PAGEOFF
  ret
.desc _cold, 0x400
_f1:
L3: adrp x0, _v@PAGE
L4: add x0, x0, _v@PAGEOFF
  ret
_f2:
L5: adrp x8, _v@PAGE
L6: ldr x0, [x8, _v@PAGEOFF]
L7: adrp x9, _v@PAGE
L8: ldr x1, [x9, _v@PAGEOFF]
  ret
_wk:
L9: adrp x8, _v@PAGE
L10: ldr x0, [x8, _v@PAGEOFF]
  ret
_span1:
L11: adrp x0, _v@PAGE
_span2:
L12: add x0, x0, _v@PAGEOFF
  ret

.section __TEXT,__text2,regular,pure_instructions
.p2align 2
.globl _a2
_a2:
L13: adrp x0, _v@PAGE
L14: add x0, x0, _v@PAGEOFF
  ret

.data
.globl _v
_v: .quad 42
L15: .long 0
L16: .long 0

.loh AdrpAdrp L5, L7
.loh AdrpLdr L7, L8
.loh AdrpLdr L5, L6
.loh AdrpAdd L13, L14
.loh AdrpAdd L3, L4
.loh AdrpAdd L1, L2
.loh AdrpLdr L9, L10
.loh AdrpAdd L11, L12
.loh AdrpAdd L15, L16
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.p2align 2
.globl _b1, _wk
.weak_definition _wk
_b1:
L1: adrp x0, _v@PAGE
L2: add x0, x0, _v@PAGEOFF
  ret
_wk:
L3: adrp x8, _v@PAGE
L4: add x0, x8, _v@PAGEOFF
  ret

.section __TEXT,__text2,regular,pure_instructions
.p2align 2
.globl _b2
_b2:
L5: adrp x0, _v@PAGE
L6: add x0, x0, _v@PAGEOFF
  ret

.loh AdrpAdd L5, L6
.loh AdrpAdd L3, L4
.loh AdrpAdd L1, L2
.subsections_via_symbols
EOF

$mold -r -arch $ARCH -o $t/r.o $t/a.o $t/b.o

# The hints, one per line: kind and addresses. The zero padding reads
# as hints of kind 0.
hints() {
  objdump --macho --link-opt-hints $1 |
    awk '$1 == "identifier" { if (s) print s; s = $2 ? $3 : "" }
      $1 == "value" { s = s " " $2 } END { if (s) print s }'
}
# A symbol's address in the output plus an offset.
at() {
  printf '0x%x' $((0x$(nm $t/r.o | awk -v s=$1 '$3 == s { print $1 }') + $2))
}
hints $t/r.o > $t/hints
cat > $t/expected <<EOF
AdrpAdd $(at _f1 0) $(at _f1 4)
AdrpAdrp $(at _f2 0) $(at _f2 8)
AdrpLdr $(at _f2 8) $(at _f2 12)
AdrpLdr $(at _f2 0) $(at _f2 4)
AdrpLdr $(at _wk 0) $(at _wk 4)
AdrpAdd $(at _b1 0) $(at _b1 4)
AdrpAdd $(at _cold 0) $(at _cold 4)
AdrpAdd $(at _a2 0) $(at _a2 4)
AdrpAdd $(at _b2 0) $(at _b2 4)
EOF
diff $t/expected $t/hints

# The command comes last, its payload between data in code and the
# symbol table, padded to 8 bytes.
otool -l $t/r.o > $t/lc
[ "$(awk '$1 == "cmd" { print $2 }' $t/lc | tr '\n' ' ')" = "LC_SEGMENT_64 LC_SYMTAB LC_BUILD_VERSION LC_DATA_IN_CODE LC_LINKER_OPTIMIZATION_HINT " ]
dataoff=$(grep -A3 LC_LINKER_OPTIMIZATION_HINT $t/lc | awk '$1 == "dataoff" { print $2 }')
datasize=$(grep -A3 LC_LINKER_OPTIMIZATION_HINT $t/lc | awk '$1 == "datasize" { print $2 }')
symoff=$(awk '$1 == "symoff" { print $2 }' $t/lc)
[ $((datasize % 8)) = 0 ]
[ $((dataoff + datasize)) = $symoff ]

# An object whose every hint is dropped as it is read adds no command,
# and neither does one whose hints all go with a coalesced-away copy.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.data
L1: .long 0
L2: .long 0
.loh AdrpAdd L1, L2
EOF
$mold -r -arch $ARCH -o $t/r2.o $t/c.o
otool -l $t/r2.o > $t/lc
not grep -q LC_LINKER_OPTIMIZATION_HINT $t/lc

cat <<EOF | $CC -o $t/d.o -c -xassembler -
.text
.p2align 2
.globl _wk
.weak_definition _wk
_wk:
  mov x0, xzr
  nop
  ret
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.text
.p2align 2
.globl _wk
.weak_definition _wk
_wk:
L1: adrp x0, _v@PAGE
L2: add x0, x0, _v@PAGEOFF
  ret
.loh AdrpAdd L1, L2
.subsections_via_symbols
EOF
$mold -r -arch $ARCH -o $t/r3.o $t/d.o $t/e.o
otool -l $t/r3.o > $t/lc3
not grep -q LC_LINKER_OPTIMIZATION_HINT $t/lc3
