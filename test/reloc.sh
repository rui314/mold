#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

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
  lea abs_sym, %edi
  call print
  ret
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 42
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 42

# GOT
cat <<'EOF' > $t/d.s
.globl main
main:
  mov ext_var@GOTPCREL(%rip), %rdi
  mov (%rdi), %edi
  call print
  ret
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 56
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 56

# Copyrel
cat <<'EOF' > $t/d.s
.globl main
main:
  mov ext_var(%rip), %edi
  call print
  ret
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 56
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 56

# Copyrel
cat <<'EOF' > $t/d.s
.globl main
main:
  mov foo(%rip), %rdi
  mov (%rdi), %edi
  call print
  ret

.data
foo:
  .quad ext_var
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 56
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 56

# PLT
cat <<'EOF' > $t/d.s
.globl main
main:
  mov $76, %edi
  call print@PLT
  ret
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 76
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 76

# PLT
cat <<'EOF' > $t/d.s
.globl main
main:
  mov $76, %edi
  lea print(%rip), %rax
  call *%rax
  ret
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -no-pie
$t/exe | grep -q 76
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s -pie
$t/exe | grep -q 76

# SIZE32
cat <<'EOF' > $t/d.s
.globl main
main:
  mov $foo+2@SIZE, %edi
  call print@PLT
  ret

.data
.globl foo
.type foo, %object
.size foo, 24
foo:
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s
$t/exe | grep -q 26

# SIZE64
cat <<'EOF' > $t/d.s
.globl main
main:
  movabs $foo+5@SIZE, %rdi
  call print64@PLT
  ret

.data
.globl foo
.type foo, %object
.size foo, 56
foo:
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/d.s
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
clang -fuse-ld=`pwd`/../mold -o $t/exe $t/c.so $t/e.o
$t/exe | grep -q 56

echo OK
