#!/bin/bash
source "$(dirname "$0")"/common.inc

# An input __DATA,__got is a section of the image like any other: of
# non-lazy pointers, its slots are named in the indirect symbol table,
# and dyld binds or slides them; of another type, its pointers are
# data. (ld-prime makes its slots entries of its own __got.) A -r output
# keeps the slots data their relocations fill: one of non-lazy pointers
# would need the indirect symbol table, and neither linker takes an
# object that has one.
if [ $ARCH = arm64 ]; then
  cat <<EOF > $t/a.s
.section __DATA,__got,non_lazy_symbol_pointers
.p2align 3
Lputs: .quad _puts
Lbar: .quad _bar
.data
.globl _bar
_bar: .long 42
.text
.globl _say, _get_bar, _say2
.p2align 2
_say:
  adrp x8, Lputs@PAGE
  ldr x8, [x8, Lputs@PAGEOFF]
  br x8
_get_bar:
  adrp x8, Lbar@PAGE
  ldr x8, [x8, Lbar@PAGEOFF]
  ldr w0, [x8]
  ret
_say2:
  adrp x8, _puts@GOTPAGE
  ldr x8, [x8, _puts@GOTPAGEOFF]
  br x8
EOF
else
  cat <<EOF > $t/a.s
.section __DATA,__got,non_lazy_symbol_pointers
.p2align 3
Lputs: .quad _puts
Lbar: .quad _bar
.data
.globl _bar
_bar: .long 42
.text
.globl _say, _get_bar, _say2
_say:
  jmpq *Lputs(%rip)
_get_bar:
  movq Lbar(%rip), %rax
  movl (%rax), %eax
  ret
_say2:
  jmpq *_puts@GOTPCREL(%rip)
EOF
fi
$CC -o $t/a.o -c $t/a.s

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <stdio.h>
void say(const char *);
void say2(const char *);
int get_bar(void);
int main() {
  say("hello");
  say2("world");
  printf("%d\n", get_bar());
}
EOF

cp $t/a.o $t/c.o
set_section_flags $t/c.o __DATA __got 2

# ld-prime fails an assertion on a slot a symbol names, as arm64 code
# names the slots it loads.
if [ $ARCH = x86_64 ] || is_mold; then
  for obj in a c; do
    $CC --ld-path=$mold -o $t/exe-$obj $t/$obj.o $t/b.o
    $RUN $t/exe-$obj > $t/out
    printf 'hello\nworld\n42\n' | cmp - $t/out
    otool -Iv $t/exe-$obj > $t/indirect
    grep -q ' _puts$' $t/indirect
    if [ $obj = a ]; then grep -q ' LOCAL$' $t/indirect; fi
  done
fi

if is_mold; then
  for obj in a c; do
    $mold -r -arch $ARCH -o $t/r.o $t/$obj.o
    otool -l $t/r.o | grep -A9 'sectname __got$' | grep -q 'flags 0x00000000'
    objdump --macho -r $t/r.o > $t/relocs
    grep -q ' _puts$' $t/relocs
    grep -q ' _bar$' $t/relocs
    $CC --ld-path=$mold -o $t/exe2 $t/r.o $t/b.o
    $RUN $t/exe2 > $t/out
    printf 'hello\nworld\n42\n' | cmp - $t/out
  done
fi
