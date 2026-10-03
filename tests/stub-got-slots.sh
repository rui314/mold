#!/bin/bash
source "$(dirname "$0")"/common.inc

# A symbol called from code gets a __stubs entry, and one whose address
# code loads a __got slot, across all libraries; with lazy binding a
# stub jumps through its __la_symbol_ptr (dyld_stub_binder takes a GOT
# slot then), otherwise through its GOT slot. The indirect symbol table
# names each entry's symbol. The entries may come in any order.
cat <<EOF | $CC -o $t/a.o -c -xc -
int zfun(void) { return 1; }
int afun(void) { return 2; }
int Zup(void) { return 3; }
int zz = 1, aa = 2;
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
int bfun(void) { return 4; }
int bb = 3;
EOF
$CC --ld-path=$mold -o $t/liba.dylib -shared $t/a.o -Wl,-install_name,@rpath/liba.dylib
$CC --ld-path=$mold -o $t/libb.dylib -shared $t/b.o -Wl,-install_name,@rpath/libb.dylib

cat <<EOF | $CC -o $t/main.o -c -xc -
#include <stdio.h>
int zfun(void), bfun(void), Zup(void), afun(void);
extern int zz, bb, aa;
int *pz(void) { return &zz; }
int *pb(void) { return &bb; }
int *pa(void) { return &aa; }
void *pp(void) { return (void *)&printf; }
int main() { puts("x"); return zfun() + bfun() + Zup() + afun() == 10 ? 0 : 1; }
EOF

# The symbols of a section's indirect symbol table entries, sorted.
slots() {
  otool -Iv $1 | awk -v want="$2" '/Indirect symbols for/ { s = index($0, "," want ")") > 0; next }
    s && $1 ~ /^0x/ { print ($3 == "" ? $2 : $3) }' | sort | tr '\n' ' '
}

if [ $ARCH = arm64 ]; then classic=11.0; else classic=12.0; fi

$CC --ld-path=$mold -o $t/exe1 $t/main.o $t/liba.dylib $t/libb.dylib \
  -Wl,-rpath,$t -mmacosx-version-min=14.0
$t/exe1 | grep -q x
[ "$(slots $t/exe1 __stubs)" = '_Zup _afun _bfun _puts _zfun ' ]
[ "$(slots $t/exe1 __got)" = '_Zup _aa _afun _bb _bfun _printf _puts _zfun _zz ' ]

$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/liba.dylib $t/libb.dylib \
  -Wl,-rpath,$t -mmacosx-version-min=$classic
$t/exe2 | grep -q x
[ "$(slots $t/exe2 __stubs)" = '_Zup _afun _bfun _puts _zfun ' ]
[ "$(slots $t/exe2 __got)" = '_aa _bb _printf _zz dyld_stub_binder ' ]
[ "$(slots $t/exe2 __la_symbol_ptr)" = '_Zup _afun _bfun _puts _zfun ' ]

# Stub i jumps through lazy pointer i, which names the same symbol.
otool -Iv $t/exe2 | awk '/Indirect symbols for/ { s = $0; next }
  s ~ /__stubs/ && $1 ~ /^0x/ { print $3 > "'$t/stubs'" }
  s ~ /__la_symbol_ptr/ && $1 ~ /^0x/ { print $3 > "'$t/lazy'" }'
diff $t/stubs $t/lazy

# Weak-lookup binds (C++ template instances coalesced across images,
# libc++'s operator new) take GOT slots too.
cat <<EOF | $CXX -o $t/c.o -c -xc++ -fno-exceptions -
#include <cstdio>
#include <new>
template <typename T> struct W { static int f() { return sizeof(T); } };
int main() { int *p = new int(W<long>::f() + W<char>::f()); std::printf("%d\n", *p); }
EOF
$CXX --ld-path=$mold -o $t/exe3 $t/c.o -mmacosx-version-min=14.0
$t/exe3 | grep -q '^9$'
[ "$(slots $t/exe3 __got)" = '__ZN1WIcE1fEv __ZN1WIlE1fEv __Znwm _printf ' ]

# With lazy binding only the lazily bound stubs have a lazy pointer and
# a stub helper entry; a weak-lookup stub jumps through its GOT slot.
$CXX --ld-path=$mold -o $t/exe5 $t/c.o -mmacosx-version-min=$classic
$t/exe5 | grep -q '^9$'
[ "$(slots $t/exe5 __stubs)" = '__ZN1WIcE1fEv __ZN1WIlE1fEv __Znwm _printf ' ]
[ "$(slots $t/exe5 __got)" = '__ZN1WIcE1fEv __ZN1WIlE1fEv __Znwm dyld_stub_binder ' ]
[ "$(slots $t/exe5 __la_symbol_ptr)" = '_printf ' ]

# A slot holding one of the image's own addresses is rebased to it.
[ $ARCH = x86_64 ] || exit 0
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.data
.globl _ldz
_ldz: .quad 1
.globl _laa
_laa: .quad 2
.text
.globl _getl
_getl:
  addq _ldz@GOTPCREL(%rip), %rax
  addq _zz@GOTPCREL(%rip), %rax
  addq _laa@GOTPCREL(%rip), %rax
  ret
EOF
$CC --ld-path=$mold -o $t/exe4 $t/main.o $t/d.o $t/liba.dylib $t/libb.dylib \
  -Wl,-rpath,$t -mmacosx-version-min=$classic
$t/exe4 | grep -q x
nm $t/exe4 > $t/nm4
laa=$(awk '$3 == "_laa" { print $1 }' $t/nm4)
ldz=$(awk '$3 == "_ldz" { print $1 }' $t/nm4)
dyld_info -fixups $t/exe4 | awk '$2 == "__got" && $4 == "rebase" { print $NF }' | sort > $t/local
printf '0x%x\n0x%x\n' 0x$laa 0x$ldz | sort > $t/expected
diff $t/expected $t/local
