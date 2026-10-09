#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -static image has no dyld to read fixups, so it gets none by
# default: a pointer holds its final address. -fixup_chains still asks
# for chains, for a loader of the image's own to walk, and ld-prime
# then writes LC_DYLD_CHAINED_FIXUPS with the rebases and weak-lookup
# binds a dynamic image would have, in the offset pointer format
# whatever the deployment target, but no export trie. The chains make
# the image PIE unless -no_pie says otherwise, and slide it in place of
# local relocations.
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
EOF

$mold -arch $ARCH -static -e __start $t/a.o -o $t/exe1
otool -l $t/exe1 > $t/lc1
not grep -q 'cmd LC_DYLD_CHAINED_FIXUPS' $t/lc1

$mold -arch $ARCH -static -e __start -fixup_chains $t/a.o -o $t/exe2
otool -l $t/exe2 > $t/lc2
grep -q 'cmd LC_DYLD_CHAINED_FIXUPS' $t/lc2
not grep -q 'cmd LC_DYLD_EXPORTS_TRIE' $t/lc2
not grep -q 'cmd LC_DYLD_INFO' $t/lc2
grep -q 'cmd LC_DYSYMTAB' $t/lc2
grep -q 'nlocrel 0$' $t/lc2
otool -hv $t/exe2 > $t/hdr2
grep -q PIE $t/hdr2
dyld_info -fixups $t/exe2 > $t/fixups2
grep -q "__data .* rebase " $t/fixups2
grep -q "__data .* bind *<weak-def-coalesce>/_wf" $t/fixups2
dyld_info -fixup_chains $t/exe2 | grep -q DYLD_CHAINED_PTR_64_OFFSET

$mold -arch $ARCH -static -e __start -fixup_chains -no_pie $t/a.o -o $t/exe3
otool -l $t/exe3 > $t/lc3
grep -q 'cmd LC_DYLD_CHAINED_FIXUPS' $t/lc3
not grep -q 'cmd LC_DYSYMTAB' $t/lc3
otool -hv $t/exe3 > $t/hdr3
not grep -q PIE $t/hdr3
