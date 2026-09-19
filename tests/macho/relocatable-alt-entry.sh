#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output must keep the n_desc flags of the symbols it re-emits.
# N_ALT_ENTRY matters most: it marks a symbol that does not start a new
# subsection. Swift's class metadata symbol ($s..CN) is an alt entry
# 0x18 bytes into the full-metadata object ($s..CMf), and code reaches
# it as CMf+0x18. A later link that took the alt entry for a subsection
# boundary re-aligned it, moved it 8 bytes away from that reference,
# and NetNewsWire's tests crashed in objc_opt_self. N_NO_DEAD_STRIP
# and N_WEAK_REF survive too.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__data
.p2align 4
.globl _full
_full:
.quad 1, 2, 3
.globl _cn
.alt_entry _cn
_cn:
.quad 4
.globl _keep
.no_dead_strip _keep
_keep:
.quad 5

.text
.globl _use_weak
.p2align 2
_use_weak:
.weak_reference _maybe
$(if [ $ARCH = arm64 ]; then echo 'adrp x0, _maybe@GOTPAGE'; echo 'ldr x0, [x0, _maybe@GOTPAGEOFF]'; echo 'ret'; else echo 'movq _maybe@GOTPCREL(%rip), %rax'; echo 'ret'; fi)
EOF
nm -m $t/a.o > $t/nm_in
grep -q 'alt entry.* _cn' $t/nm_in
grep -q 'no dead strip.* _keep' $t/nm_in
grep -q 'weak.* _maybe' $t/nm_in

$mold -r -arch $ARCH -o $t/r.o $t/a.o
nm -m $t/r.o > $t/nm
grep -q '\[alt entry\].* _cn' $t/nm
grep -q '\[no dead strip\].* _keep' $t/nm
grep -q 'weak.* _maybe' $t/nm

# The alt entry stays 24 bytes into _full through a final link with
# subsections (an undefined weak reference with no definition anywhere
# is an error for ld64 too, so main defines it).
cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
extern long full[];
extern long cn;
extern long keep;
long maybe = 9;
long *use_weak(void);
int main() {
  printf("%ld %ld %ld %ld\n", &cn - full, cn, &keep - full, *use_weak());
}
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -Wl,-dead_strip
$t/exe | grep -q '^3 4 4 9$'
