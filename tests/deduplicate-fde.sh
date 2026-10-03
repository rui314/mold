#!/bin/bash
source "$(dirname "$0")"/common.inc

# When -deduplicate folds copies of a function, each with its own FDE,
# together, the survivor keeps its FDE and the copies' go: whether the
# copies are in objects of their own or in one a -r link made, all of
# one name. (ld-prime keeps every copy's FDE in the latter case, each
# with an __unwind_info entry at the survivor's address.)
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
addr=$(nm $t/exe | awk '/ _helper$/ { print $1; exit }')
[ "$(dwarfdump --eh-frame $t/exe | grep -c " FDE .* pc=$(echo $addr | sed 's/^0*//')\\.")" = 1 ]
objdump --macho --unwind-info $t/exe > $t/unwind
off=$(printf '0x%08x' $((0x$addr - 0x100000000)))
[ "$(grep -c "function offset=$off" $t/unwind)" = 1 ]

# main's unwind record, after the copies', keeps its entry.
$CC --ld-path=$mold -o $t/exe2 $t/a1.o $t/a2.o $t/a3.o $t/main.o -Wl,-deduplicate
$t/exe2
[ "$(dwarfdump --eh-frame $t/exe2 | grep -c ' FDE ')" = 1 ]
addr=$(nm $t/exe2 | awk '/ _main$/ { print $1; exit }')
off=$(printf '0x%08x' $((0x$addr - 0x100000000)))
objdump --macho --unwind-info $t/exe2 > $t/unwind2
grep "function offset=$off," $t/unwind2 | grep -qv '=0x00000000$'
