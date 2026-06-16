#!/bin/bash
. $(dirname $0)/common.inc

# When mold relaxes an instruction sequence, the relocations attached to the
# original instructions must stay consistent with the relaxed code for
# --emit-relocs.
#
# A GOTPCRELX relaxation turns a GOT load into a single PC-relative
# instruction, which has a matching relocation type, so we rewrite it (as GNU
# ld does). A TLS relaxation rewrites a whole recognized instruction sequence
# that has no single matching relocation, so the TLS relocation type is kept on
# the rewritten sequence as a marker (as GNU ld does).

#
# GOTPCRELX: a GOT load of a link-time-constant address is relaxed from
#   mov foo@GOTPCREL(%rip), %rax   =>   lea foo(%rip), %rax
# so R_X86_64_REX_GOTPCRELX is rewritten to R_X86_64_PC32.
#
cat <<'EOF' | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
_start:
  movq foo@GOTPCREL(%rip), %rax
  movl (%rax), %eax
  cmpl $42, %eax            # exit(0) iff the relaxed lea resolved to foo
  jne 1f
  xorl %edi, %edi
  jmp 2f
1:
  movl $1, %edi
2:
  movl $60, %eax            # __NR_exit
  syscall
.data
.hidden foo
foo:
  .long 42
EOF

$CC -B. -o $t/exe1 $t/a.o -static -nostdlib -Wl,-e,_start,--emit-relocs
$QEMU $t/exe1

$OBJDUMP -dr $t/exe1 > $t/exe1.objdump

# The GOT load was relaxed to a PC-relative lea of foo.
grep -E 'lea.*<foo>' $t/exe1.objdump

# The relocation was rewritten to match the lea, and it sits on the lea
# (objdump prints a relocation right after the instruction it applies to).
grep -Fw R_X86_64_PC32 $t/exe1.objdump
not grep -Fw R_X86_64_REX_GOTPCRELX $t/exe1.objdump
awk '
  /R_X86_64_PC32/ { if (prev !~ /lea/) { bad=1; print "not on lea: " prev }; next }
  { prev = $0 }
  END { exit bad+0 }
' $t/exe1.objdump

#
# TLS general-dynamic of a local symbol is relaxed to local-exec:
#   lea gd@tlsgd(%rip),%rdi; call __tls_get_addr   =>   mov %fs:0,%rax; ...
# The whole sequence is rewritten, so R_X86_64_TLSGD is kept as a marker.
#
cat <<'EOF' | $CC -fPIC -o $t/b.o -c -xc -
__thread int gd;
int main(void) { return gd; }   // gd is zero-initialized, so exits 0
EOF

$CC -B. -o $t/exe2 $t/b.o -no-pie -Wl,--emit-relocs
$QEMU $t/exe2

$OBJDUMP -dr $t/exe2 > $t/exe2.objdump

# The general-dynamic sequence was relaxed to a local-exec access via %fs.
grep -F %fs:0x0 $t/exe2.objdump

# The TLS relocation is kept and still names the symbol.
grep -E $'R_X86_64_TLSGD[ \t]+gd' $t/exe2.objdump
