#!/bin/bash
source "$(dirname "$0")"/common.inc

# A class reference slot folded into the GOT (macOS 15 on) is its
# class's GOT entry, which ld-prime makes an atom of its own
# stubs-got-file, and so a text relocation to the slot names it
# anon-N. It makes those atoms as it meets the references, object by
# object in address order: two for each GOT entry, and one for each
# stub, after its GOT entry's. Here puts's GOT entry and stub take
# anon-0 to anon-2, NSString's entry anon-3 and NSObject's anon-5.
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

not $CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -mmacosx-version-min=15.0 \
  2> $t/log
grep -q "text-relocation in '_ptr' (.*/a.o) to 'anon-5'" $t/log
grep -q "text-relocation in '_ptr'+0x8 (.*/a.o) to 'anon-3'" $t/log

# The objects' order is the references'.
not $CC --ld-path=$mold -o $t/exe $t/b.o $t/a.o -framework Foundation \
  -mmacosx-version-min=15.0 2> $t/log
grep -q "text-relocation in '_ptr' (.*/a.o) to 'anon-0'" $t/log
grep -q "text-relocation in '_ptr'+0x8 (.*/a.o) to 'anon-5'" $t/log

# Below macOS 15 a slot stays in place, an atom of its object that no
# label names, whatever its labels: in a.o, _main is anon-0, LCR1
# anon-1 and LCR2 anon-2.
not $CC --ld-path=$mold -o $t/exe $t/b.o $t/a.o -framework Foundation \
  -mmacosx-version-min=14.0 2> $t/log
grep -q "text-relocation in '_ptr' (.*/a.o) to 'anon-1'" $t/log
grep -q "text-relocation in '_ptr'+0x8 (.*/a.o) to 'anon-2'" $t/log

# A slot coalesced into an equal one is that one. Without subsections
# an arm64 assembler's ltmpN counts too, after the slot's atom (as
# after a literal's): ltmp0 and _main are anon-0 and anon-1, _other
# anon-2, then LCR1 anon-3, before ltmp1.
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
if [ $ARCH = arm64 ]; then
  grep -q "text-relocation in '_ptr' (.*/c.o) to 'anon-3'" $t/log
  grep -q "text-relocation in '_ptr'+0x8 (.*/c.o) to 'anon-6'" $t/log
  grep -q "text-relocation in '_ptr'+0x10 (.*/c.o) to 'anon-3'" $t/log
else
  grep -q "text-relocation in '_ptr' (.*/c.o) to 'anon-2'" $t/log
  grep -q "text-relocation in '_ptr'+0x8 (.*/c.o) to 'anon-4'" $t/log
  grep -q "text-relocation in '_ptr'+0x10 (.*/c.o) to 'anon-2'" $t/log
fi
