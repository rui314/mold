#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF | cc -o $t/a.o -c -x assembler -
  .text
  .globl main
main:
  lea msg(%rip), %rdi
  xor %rax, %rax
  call printf@PLT
  xor %rax, %rax
  ret

  .data
msg:
  .string "Hello world\n"
EOF

cat <<EOF > $t/script
GROUP($t/a.o)
EOF

clang -fuse-ld=`pwd`/../mold -o $t/exe $t/script
$t/exe | grep -q 'Hello world'

clang -fuse-ld=`pwd`/../mold -o $t/exe -Wl,-T,$t/script
$t/exe | grep -q 'Hello world'

clang -fuse-ld=`pwd`/../mold -o $t/exe -Wl,--script,$t/script
$t/exe | grep -q 'Hello world'

echo OK
