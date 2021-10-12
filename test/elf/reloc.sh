#!/bin/bash
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

[ $(uname -m) = x86_64 ] || { echo skipped; exit; }

cat <<'EOF' | cc -fPIC -c -o $t/a.o -x assembler -
.data
.globl ext_var
.type ext_var, @object
.size ext_var, 56
ext_var:
  .long 56
EOF

cat <<'EOF' | cc -fPIC -c -o $t/b.o -xc -
#include <stdio.h>

int print(int x) {
  printf("%d\n", x);
  return 0;
}

int print64(long x) {
  printf("%ld\n", x);
  return 0;
}
EOF

cc -shared -o $t/c.so $t/a.o $t/b.o

# Absolute symbol
cat <<'EOF' > $t/d.s
.globl abs_sym
.set abs_sym, 42

.globl main
main:
  sub $8, %rsp
  lea abs_sym, %edi
  call print
  add $8, %rsp
  ret
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 42
clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 42

# GOT
cat <<'EOF' > $t/d.s
.globl main
main:
  sub $8, %rsp
  mov ext_var@GOTPCREL(%rip), %rdi
  mov (%rdi), %edi
  call print
  add $8, %rsp
  ret
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 56
clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 56

# Copyrel
cat <<'EOF' > $t/d.s
.globl main
main:
  sub $8, %rsp
  mov ext_var(%rip), %edi
  call print
  add $8, %rsp
  ret
EOF

clang -c -o $t/d.o $t/d.s
clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.o -no-pie
$t/exe | grep -q 56
clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 56

# Copyrel
cat <<'EOF' > $t/d.s
.globl main
main:
  sub $8, %rsp
  mov foo(%rip), %rdi
  mov (%rdi), %edi
  call print
  add $8, %rsp
  ret

.data
foo:
  .quad ext_var
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 56
clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 56

# PLT
cat <<'EOF' > $t/d.s
.globl main
main:
  sub $8, %rsp
  mov $76, %edi
  call print@PLT
  add $8, %rsp
  ret
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 76
clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 76

# PLT
cat <<'EOF' > $t/d.s
.globl main
main:
  sub $8, %rsp
  mov $76, %edi
  lea print(%rip), %rax
  call *%rax
  add $8, %rsp
  ret
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 76
clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 76

# SIZE32
cat <<'EOF' > $t/d.s
.globl main
main:
  sub $8, %rsp
  mov $foo+2@SIZE, %edi
  call print@PLT
  add $8, %rsp
  ret

.data
.globl foo
.type foo, %object
.size foo, 24
foo:
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s
$t/exe | grep -q 26

# SIZE64
cat <<'EOF' > $t/d.s
.globl main
main:
  sub $8, %rsp
  movabs $foo+5@SIZE, %rdi
  call print64@PLT
  add $8, %rsp
  ret

.data
.globl foo
.type foo, %object
.size foo, 56
foo:
EOF

clang -fuse-ld=$mold -o $t/exe $t/c.so $t/d.s
$t/exe | grep -q 61

# GOTPCREL64
cat <<'EOF' > $t/e.c
extern long ext_var;
void print64(long);

int main() {
  print64(ext_var);
}
EOF

clang -c -o $t/e.o $t/e.c -mcmodel=large -fPIC
clang -fuse-ld=$mold -o $t/exe $t/c.so $t/e.o
$t/exe | grep -q 56

# R_X86_64_32 against non-alloc section
cat <<'EOF' > $t/f.s
.globl main
main:
  sub $8, %rsp
  add $8, %rsp
  ret

.section .foo, "", @progbits
.zero 16
foo:
.quad bar

.section .bar, "", @progbits
.zero 24
bar:
.quad foo
EOF

clang -c -o $t/f.o $t/f.s
clang -fuse-ld=$mold -o $t/exe $t/f.o
readelf -x .foo -x .bar $t/exe > $t/log

fgrep -q '0x00000010 00000000 00000000 10000000 00000000' $t/log
fgrep -q '0x00000010 18000000 00000000' $t/log

echo OK
