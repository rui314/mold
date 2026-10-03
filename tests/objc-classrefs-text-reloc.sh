#!/bin/bash
source "$(dirname "$0")"/common.inc

# A pointer in a read-only section to a class reference slot needs a
# fixup, so it is a text relocation the link refuses, at any deployment
# target; the slot goes by its label (an x86-64 assembler's relocation
# names its section and offset). (From macOS 15 on ld-prime folds the
# slot into its class's GOT entry and names that by its own numbering of
# the subsections of its "stubs-got-file", "anon-N"; a slot in place it
# names by the object's subsections'.)
if [ $ARCH = arm64 ]; then
  load() { printf 'adrp x0, %s@PAGE\n  ldr x0, [x0, %s@PAGEOFF]\n' $1 $1; }
  call='bl _puts'
else
  load() { printf 'movq %s(%%rip), %%rax\n' $1; }
  call='call _puts'
fi

cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  $call
  $(load LCR2)
  $(load LCR1)
  ret
.section __DATA,__objc_classrefs,regular,no_dead_strip
.p2align 3
LCR1:
  .quad _OBJC_CLASS_\$_NSObject
LCR2:
  .quad _OBJC_CLASS_\$_NSString
.section __TEXT,__const
.p2align 3
.globl _ptr
_ptr:
  .quad LCR1
  .quad LCR2
.subsections_via_symbols
EOF

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _f
.p2align 2
_f:
  $(load LCR)
  ret
.section __DATA,__objc_classrefs,regular,no_dead_strip
.p2align 3
LCR:
  .quad _OBJC_CLASS_\$_NSObject
.subsections_via_symbols
EOF

for min in 15.0 14.0; do
  not $CC --ld-path=$mold -o $t/exe $t/b.o $t/a.o -framework Foundation \
    -mmacosx-version-min=$min 2> $t/log
  grep -q "text-relocation in '_ptr' (.*/a.o) to '\(LCR1\|__DATA,__objc_classrefs+0x0\)'" $t/log
  grep -q "text-relocation in '_ptr'+0x8 (.*/a.o) to '\(LCR2\|__DATA,__objc_classrefs+0x8\)'" $t/log
done

# So is one to a slot coalesced into an equal one, in an object without
# subsections.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.text
.globl _main
.p2align 2
_main:
  ret
_other:
  ret
.section __DATA,__objc_classrefs,regular,no_dead_strip
.p2align 3
LCR1:
  .quad _OBJC_CLASS_\$_NSObject
LCR2:
  .quad _OBJC_CLASS_\$_NSObject
LCR3:
  .quad _OBJC_CLASS_\$_NSString
.section __TEXT,__const
.p2align 3
.globl _ptr
_ptr:
  .quad LCR2
  .quad LCR3
  .quad LCR1
EOF

not $CC --ld-path=$mold -o $t/exe $t/c.o -framework Foundation -mmacosx-version-min=14.0 \
  2> $t/log
grep -q "text-relocation in '_ptr' (.*/c.o) to " $t/log
grep -q "text-relocation in '_ptr'+0x8 (.*/c.o) to " $t/log
grep -q "text-relocation in '_ptr'+0x10 (.*/c.o) to " $t/log
