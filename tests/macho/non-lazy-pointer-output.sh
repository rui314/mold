#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime knows a section of non-lazy symbol pointers by its type,
# whatever its name: a final image keeps its type, naming each slot's
# symbol in the indirect symbol table (or INDIRECT_SYMBOL_LOCAL for a
# pointer it fills in itself), moves it from __DATA to __DATA_CONST
# as it does the GOT, and aligns each slot to a pointer. A kext's are
# plain data.
if [ $ARCH = arm64 ]; then
  cat <<EOF > $t/a.s
.data
.byte 1
.section __DATA,__foo,non_lazy_symbol_pointers
Lputs: .quad _puts
Lbar: .quad _bar
.section __FOO,__bar,non_lazy_symbol_pointers
Lputs2: .quad _puts
.data
.globl _bar
_bar: .long 42
.text
.globl _say, _say2, _get_bar
.p2align 2
_say:
  adrp x8, Lputs@PAGE
  ldr x8, [x8, Lputs@PAGEOFF]
  br x8
_say2:
  adrp x8, Lputs2@PAGE
  ldr x8, [x8, Lputs2@PAGEOFF]
  br x8
_get_bar:
  adrp x8, Lbar@PAGE
  ldr x8, [x8, Lbar@PAGEOFF]
  ldr w0, [x8]
  ret
.subsections_via_symbols
EOF
else
  cat <<EOF > $t/a.s
.data
.byte 1
.section __DATA,__foo,non_lazy_symbol_pointers
Lputs: .quad _puts
Lbar: .quad _bar
.section __FOO,__bar,non_lazy_symbol_pointers
Lputs2: .quad _puts
.data
.globl _bar
_bar: .long 42
.text
.globl _say, _say2, _get_bar
_say:
  jmpq *Lputs(%rip)
_say2:
  jmpq *Lputs2(%rip)
_get_bar:
  movq Lbar(%rip), %rax
  movl (%rax), %eax
  ret
.subsections_via_symbols
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

$CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o
$RUN $t/exe > $t/out
printf 'hello\nworld\n42\n' | cmp - $t/out

otool -l $t/exe > $t/lc
grep -A10 'sectname __foo$' $t/lc > $t/foo
grep -q 'segname __DATA_CONST$' $t/foo
grep -q 'align 2^3 (8)' $t/foo
grep -q 'flags 0x00000006' $t/foo
grep -A10 'sectname __bar$' $t/lc > $t/bar
grep -q 'segname __FOO$' $t/bar
grep -q 'flags 0x00000006' $t/bar

otool -Iv $t/exe > $t/indirect
grep -A3 '(__DATA_CONST,__foo) 2 entries' $t/indirect > $t/foo-indirect
grep -q ' _puts$' $t/foo-indirect
grep -q ' LOCAL$' $t/foo-indirect
grep -A2 '(__FOO,__bar) 1 entries' $t/indirect | grep -q ' _puts$'

# Only data that needs no writes after fixups goes to __DATA_CONST.
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/b.o -Wl,-no_data_const
$RUN $t/exe2 | cmp - $t/out
otool -l $t/exe2 | grep -A10 'sectname __foo$' > $t/foo2
grep -q 'segname __DATA$' $t/foo2
grep -q 'flags 0x00000006' $t/foo2
otool -Iv $t/exe2 | grep -q '(__DATA,__foo) 2 entries'

# A kext's are plain data.
cat <<EOF > $t/c.s
.section __DATA,__foo,non_lazy_symbol_pointers
.p2align 3
.quad _bar
.data
.globl _bar
_bar: .long 42
.subsections_via_symbols
EOF
$CC -o $t/c.o -c $t/c.s
$mold -arch $ARCH -kext -o $t/kext $t/c.o
otool -l $t/kext | grep -A10 'sectname __foo$' | grep -q 'flags 0x00000000'

# A -r output keeps the type and the relocations, and so is pointers
# to a later link (ld-prime names the slots in the indirect symbol
# table instead, which no link takes as input).
if $mold -v 2>&1 | grep -q mold-macho; then
  $mold -r -arch $ARCH -o $t/r.o $t/a.o
  otool -l $t/r.o | grep -A10 'sectname __foo$' | grep -q 'flags 0x00000006'
  objdump --macho -r $t/r.o | grep -q ' _puts$'
  $CC --ld-path=$mold -o $t/exe3 $t/r.o $t/b.o
  $RUN $t/exe3 | cmp - $t/out
fi
