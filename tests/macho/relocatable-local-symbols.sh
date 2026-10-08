#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output lists every object's local symbols - its labels, a
# zerofill one too, and the private externals it demotes to locals
# that keep N_PEXT - and its externals, and a program links with it as
# with the objects, with either linker. (ld-prime lists the locals by
# section and address, and the names at one address by rank.)
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.p2align 2
.globl _p1
.private_extern _p1
_p1: ret
zz1:
aa1:
 ret
t1_first: ret
.zerofill __DATA,__bss,b1,8,3
.data
d1_second: .quad 1
.text
.globl _f1
_f1: ret
.subsections_via_symbols
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
d2: .quad 2
.text
.p2align 2
t2: ret
.globl _p2
.private_extern _p2
_p2: ret
.globl _f2
_f2: ret
.subsections_via_symbols
EOF
$mold -arch $ARCH -r $t/a.o $t/b.o -o $t/r.o

names() { nm -p "$@" | awk 'NF == 3 {print $3}' | sort; }
[ "$(names $t/a.o $t/b.o)" = "$(names $t/r.o)" ]
nm -m $t/r.o > $t/nm
grep -q '(__TEXT,__text) non-external (was a private external) _p1$' $t/nm
grep -q '(__TEXT,__text) non-external (was a private external) _p2$' $t/nm
grep -q '(__DATA,__bss) non-external b1$' $t/nm
grep -q '(__TEXT,__text) external _f1$' $t/nm
grep -q '(__TEXT,__text) external _f2$' $t/nm

cat <<EOF | $CC -o $t/main.o -c -xc -
void f1(void), f2(void);
int main() { f1(); f2(); return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o
$RUN $t/exe
$CC -o $t/exe2 $t/main.o $t/r.o
$RUN $t/exe2
