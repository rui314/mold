#!/bin/bash
source "$(dirname "$0")"/common.inc

# A section-relative relocation may point one past the end of a
# section: the assembler's DWARF for a -g assembly file (BoringSSL's
# hand-written routines) sets DW_AT_high_pc to the end of __text, and
# a compiler's __debug_ranges ends there too (Transmission's libutp).
# The reader rejected those as "bad relocation". A final link discards
# the DWARF sections before reading their relocations; a -r link,
# which Xcode's prelink of a package target is, keeps them.
if [ $ARCH = arm64 ]; then
  body='mov w0, #5'
else
  body='movl $5, %eax'
fi
# The 128-byte-aligned __const after __text leaves a gap, so the
# address one past __text is inside no section (BoringSSL's layout).
cat <<EOF | $CC -g -o $t/a.o -c -x assembler -
.text
.globl _f
_f:
  $body
  ret
Lend:
.section __TEXT,__const
.p2align 7
  .quad 1
.section __DATA,__data
.p2align 3
.globl _end_ptr
_end_ptr:
  .quad Lend
EOF
otool -rv $t/a.o | grep -q 'debug_info'

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int f(void);
extern char *end_ptr;
int main() {
  printf("%d %s\n", f(), end_ptr > (char *)f && end_ptr - (char *)f <= 16 ? "ok" : "bad");
}
EOF

$mold -r -arch $ARCH -o $t/r.o $t/a.o
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o
$t/exe | grep -q '^5 ok$'

# Empty sections (an emptied coverage section, say) share a file
# offset with a neighbor and must not confuse the copy.
cat <<EOF | $CC -o $t/b.o -c -x assembler -
.section __DATA,__zero_one
.section __DATA,__zero_two
EOF
cat <<EOF | $CC -o $t/c.o -c -xc -
_Thread_local int tls_var = 3;
int get_tls(void) { return tls_var; }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a.o $t/b.o $t/c.o
$t/exe2 | grep -q '^5 ok$'
