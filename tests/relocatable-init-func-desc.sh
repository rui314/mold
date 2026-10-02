#!/bin/bash
source "$(dirname "$0")"/common.inc

# Dead stripping keeps the initializer and terminator pointer lists by
# their section type, whatever their attributes and symbols say, in a
# link of a -r output as in one of the objects. The entries' labels -
# aliases of a list entry, and externals - carry through with the
# n_desc the object gave them. (arm64 dyld runs no terminators.)
cat > $t/b.s <<EOF
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
ca:
cb:
  .quad _i1
.globl _cg
_cg:
cl:
  .quad _i1
.section __DATA,__mod_term_func,mod_term_funcs
.p2align 3
ta:
tb:
  .quad _i1
.data
.p2align 3
da:
db:
  .quad 1
EOF
$CC -o $t/b.o -c $t/b.s
cat $t/b.s > $t/c.s
echo .subsections_via_symbols >> $t/c.s
$CC -o $t/c.o -c $t/c.s

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
void i1(void) { printf("i1 "); }
int main() { printf("main "); }
EOF

for obj in b c; do
  $mold -arch $ARCH -r $t/$obj.o -o $t/r$obj.o
  nm -m $t/r$obj.o > $t/nm$obj
  for s in ca cb cl ta tb da db; do
    grep -q "non-external $s\$" $t/nm$obj
  done
  grep -q 'external _cg$' $t/nm$obj
  $CC --ld-path=$mold -o $t/exe0$obj $t/main.o $t/$obj.o -Wl,-dead_strip
  $t/exe0$obj > $t/out0$obj
  grep -q '^i1 i1 main ' $t/out0$obj
  $CC --ld-path=$mold -o $t/exe$obj $t/main.o $t/r$obj.o -Wl,-dead_strip
  [ "$($t/exe$obj)" = "$(cat $t/out0$obj)" ]
  otool -l $t/exe$obj | grep -q 'sectname __mod_term_func'
  $CC -o $t/exe2$obj $t/main.o $t/r$obj.o -Wl,-dead_strip
  [ "$($t/exe2$obj)" = "$(cat $t/out0$obj)" ]
done
