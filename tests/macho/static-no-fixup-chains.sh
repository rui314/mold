#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -static image has no dyld to read fixups, so it gets none by
# default. -no_fixup_chains asks for them in the classic format:
# LC_DYLD_INFO_ONLY with a rebase for every pointer and a weak bind
# for each slot holding one of the image's weak definitions, but no
# export trie, whether or not the image is PIE. Under -pie the rebases
# replace the local relocations, and LC_DYSYMTAB lists none.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl __start
.globl _wf
.weak_definition _wf
.p2align 2
__start:
  ret
_wf:
  ret
.data
.globl _p
.p2align 3
_p: .quad __start
_q: .quad _wf
_r: .quad _p
EOF

$mold -arch $ARCH -static -e __start -no_fixup_chains $t/a.o -o $t/exe1
otool -l $t/exe1 > $t/lc1
grep -q 'cmd LC_DYLD_INFO_ONLY' $t/lc1
grep -q 'export_size 0$' $t/lc1
not grep -q 'cmd LC_DYSYMTAB' $t/lc1
not grep -q 'cmd LC_DYLD_CHAINED_FIXUPS' $t/lc1
otool -hv $t/exe1 > $t/hdr1
not grep -q PIE $t/hdr1
objdump --macho --rebase $t/exe1 > $t/rebase1
[ "$(grep -c '__data .* pointer$' $t/rebase1)" = 3 ]
objdump --macho --weak-bind $t/exe1 > $t/weak1
grep -q '__data .* pointer  *0  *_wf$' $t/weak1

$mold -arch $ARCH -static -e __start -no_fixup_chains -pie $t/a.o -o $t/exe2
otool -l $t/exe2 > $t/lc2
grep -q 'cmd LC_DYLD_INFO_ONLY' $t/lc2
grep -q 'cmd LC_DYSYMTAB' $t/lc2
grep -q 'nlocrel 0$' $t/lc2
otool -hv $t/exe2 > $t/hdr2
grep -q PIE $t/hdr2
objdump --macho --rebase $t/exe2 > $t/rebase2
[ "$(grep -c '__data .* pointer$' $t/rebase2)" = 3 ]
objdump --macho --weak-bind $t/exe2 > $t/weak2
grep -q '__data .* pointer  *0  *_wf$' $t/weak2

$mold -arch $ARCH -static -e __start $t/a.o -o $t/exe3
otool -l $t/exe3 > $t/lc3
not grep -q 'cmd LC_DYLD_INFO' $t/lc3

# kmutil slides a -kernel image by its local relocations and links a
# kext by its relocations; ld-prime takes neither -no_fixup_chains nor
# -fixup_chains for them.
for opt in -no_fixup_chains -fixup_chains; do
  $mold -arch $ARCH -static -kernel -e __start $opt $t/a.o -o $t/kernel$opt
  otool -l $t/kernel$opt > $t/lc-kernel$opt
  not grep -q 'cmd LC_DYLD_' $t/lc-kernel$opt
  not grep -q 'nlocrel 0$' $t/lc-kernel$opt

  $mold -arch $ARCH -kext $opt $t/a.o -o $t/kext$opt
  otool -l $t/kext$opt > $t/lc-kext$opt
  not grep -q 'cmd LC_DYLD_' $t/lc-kext$opt
  not grep -q 'nlocrel 0$' $t/lc-kext$opt
done
