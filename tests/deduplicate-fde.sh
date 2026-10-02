#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r link may put copies of one local function, each with its own FDE,
# in one object, all of one name. When -deduplicate folds them together,
# ld-prime keeps every copy's FDE and unwind record, all at the
# survivor's address and the records all pointing at the last FDE; it
# drops them for copies of other names, or in other objects.
if [ $ARCH = arm64 ]; then
  prologue() { echo 'stp x29, x30, [sp, #-16]!'; echo '.cfi_def_cfa_offset 16'; }
  body() { echo 'mov w0, #7'; echo 'ldp x29, x30, [sp], #16'; echo ret; }
  jump() { echo "b $1"; }
else
  prologue() { echo 'pushq %rbp'; echo '.cfi_def_cfa_offset 16'; }
  body() { echo 'movl $7, %eax'; echo 'popq %rbp'; echo ret; }
  jump() { echo "jmp $1"; }
fi

for i in 1 2 3; do
  {
    echo '.subsections_via_symbols'
    echo '.text'
    echo ".globl _call$i"
    echo '.p2align 2'
    echo "_call$i:"
    jump _helper
    echo '.p2align 2'
    echo '_helper:'
    echo '.cfi_startproc'
    prologue
    # An escape the compact encoding can't express keeps the FDE.
    echo '.cfi_escape 0x2e, 0x10'
    body
    echo '.cfi_endproc'
  } > $t/a$i.s
  $CC -o $t/a$i.o -c $t/a$i.s
done
$mold -r -arch $ARCH -o $t/r.o $t/a1.o $t/a2.o $t/a3.o

cat <<EOF | $CC -o $t/main.o -c -xc -
int call1(void), call2(void), call3(void);
int main() { return call1() + call2() + call3() != 21; }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -Wl,-deduplicate
$t/exe
addr=$(nm $t/exe | awk '/ _helper$/ { print $1 }')
[ "$(dwarfdump --eh-frame $t/exe | grep -c " FDE .* pc=$(echo $addr | sed 's/^0*//')\\.")" = 3 ]
objdump --macho --unwind-info $t/exe > $t/unwind
off=$(printf '0x%08x' $((0x$addr - 0x100000000)))
[ "$(grep -c "function offset=$off" $t/unwind)" = 3 ]

# Separate objects keep the survivor's FDE alone.
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a1.o $t/a2.o $t/a3.o -Wl,-deduplicate
$t/exe2
[ "$(dwarfdump --eh-frame $t/exe2 | grep -c ' FDE ')" = 1 ]
