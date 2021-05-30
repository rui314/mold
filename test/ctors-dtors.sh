#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<'EOF' | clang -c -o $t/a.o -x assembler -
.globl ctor1, ctor2, dtor1, dtor2
ctor1:
  mov $97, %edi
  call putchar
  ret
dtor1:
  mov $98, %edi
  call putchar
  ret

.section .ctors,"aw",@progbits
.quad ctor1

.section .dtors,"aw",@progbits
.quad dtor1
EOF

cat <<EOF | clang -c -o $t/b.o -xc -
int main() {}
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/a.o $t/b.o
$t/exe | grep -q ab

echo OK
