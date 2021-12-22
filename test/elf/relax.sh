#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

# Skip if target is not x86-64
[ "$(uname -m)" = x86_64 ] || { echo skipped; exit; }

cat <<EOF | clang -o "$t"/a.o -c -x assembler -Wa,-mrelax-relocations=yes -
.globl bar
bar:
  mov foo@GOTPCREL(%rip), %rax
  mov foo@GOTPCREL(%rip), %rcx
  mov foo@GOTPCREL(%rip), %rdx
  mov foo@GOTPCREL(%rip), %rbx
  mov foo@GOTPCREL(%rip), %rbp
  mov foo@GOTPCREL(%rip), %rsi
  mov foo@GOTPCREL(%rip), %rdi
  mov foo@GOTPCREL(%rip), %r8
  mov foo@GOTPCREL(%rip), %r9
  mov foo@GOTPCREL(%rip), %r10
  mov foo@GOTPCREL(%rip), %r11
  mov foo@GOTPCREL(%rip), %r12
  mov foo@GOTPCREL(%rip), %r13
  mov foo@GOTPCREL(%rip), %r14
  mov foo@GOTPCREL(%rip), %r15

  call *foo@GOTPCREL(%rip)
  jmp  *foo@GOTPCREL(%rip)
EOF

cat <<EOF | clang -o "$t"/b.o -c -xc -
void foo() {}

int main() {
  return 0;
}
EOF

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o
objdump -d "$t"/exe | grep -A20 '<bar>:' > "$t"/log

grep -Pq 'lea \s*0x.+\(%rip\),%rax .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%rcx .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%rdx .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%rbx .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%rbp .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%rsi .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%rdi .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%r8  .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%r9  .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%r10 .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%r11 .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%r12 .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%r13 .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%r14 .*<foo>' "$t"/log
grep -Pq 'lea \s*0x.+\(%rip\),%r15 .*<foo>' "$t"/log
grep -Pq 'call.*<foo>' "$t"/log
grep -Pq 'jmp.*<foo>' "$t"/log

clang -fuse-ld="$mold" -o "$t"/exe "$t"/a.o "$t"/b.o -Wl,-no-relax
objdump -d "$t"/exe | grep -A20 '<bar>:' > "$t"/log

grep -Pq 'mov \s*0x.+\(%rip\),%rax' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%rcx' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%rdx' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%rbx' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%rbp' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%rsi' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%rdi' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%r8 ' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%r9 ' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%r10' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%r11' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%r12' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%r13' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%r14' "$t"/log
grep -Pq 'mov \s*0x.+\(%rip\),%r15' "$t"/log
grep -Pq 'call.*\(%rip\)' "$t"/log
grep -Pq 'jmp.*\(%rip\)' "$t"/log

echo OK
