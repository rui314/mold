#!/bin/bash
set -e
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

echo OK
