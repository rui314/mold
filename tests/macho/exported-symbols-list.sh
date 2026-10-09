#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int foo() { return 1; }
int bar() { return 2; }
int main() { return 0; }
EOF2

cat <<EOF2 > $t/list
# only foo is exported
_foo
EOF2

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-exported_symbols_list,$t/list
dyld_info -exports $t/exe > $t/exports
grep -q _foo $t/exports
not grep -q _bar $t/exports

# ld-prime turns what the list leaves out into a private extern: a
# local in the symbol table, not only a name missing from the trie.
nm -m $t/exe > $t/syms-exe
grep -q 'non-external (was a private external) _bar' $t/syms-exe
grep -q 'non-external (was a private external) __mh_execute_header' $t/syms-exe

# _main is demoted too when the list omits it; _foo stays external.
grep -q 'non-external (was a private external) _main' $t/syms-exe
grep -q ' external _foo' $t/syms-exe

# -unexported_symbols_list demotes only what it names.
echo _bar > $t/unlist
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-unexported_symbols_list,$t/unlist
dyld_info -exports $t/exe2 > $t/exports2
grep -q _foo $t/exports2
not grep -q _bar $t/exports2
nm -m $t/exe2 > $t/nm2
grep -q 'non-external (was a private external) _bar' $t/nm2
grep -q ' external _main' $t/nm2
grep -q ' external __mh_execute_header' $t/nm2

cat <<EOF2 | $CC -o $t/b.o -c -xc -
__attribute__((weak)) int wf(void) { return 1; }
int g(void) { return wf(); }
int h(void) { return 2; }
EOF2

# A dylib's hidden definition is no dead-strip root.
$CC --ld-path=$mold -dynamiclib -o $t/libb.dylib $t/b.o \
  -Wl,-exported_symbol,_g,-dead_strip
nm $t/libb.dylib > $t/syms
not grep -q _h $t/syms

# A hidden weak definition is not coalesced: no weak-lookup bind, and
# neither WEAK_DEFINES nor BINDS_TO_WEAK.
$CC --ld-path=$mold -dynamiclib -o $t/libc.dylib $t/b.o \
  -Wl,-unexported_symbol,_wf
[ "$(otool -h $t/libc.dylib | tail -1 | awk '{print $NF}')" = 0x00100085 ]
dyld_info -fixups $t/libc.dylib > $t/fixups
not grep -q weak-def-coalesce $t/fixups

$mold -arch $ARCH -r $t/b.o -exported_symbol _g -o $t/c.o
nm -m $t/c.o > $t/syms
grep -q 'non-external (was a private external) _h' $t/syms

# ld-prime's patterns: a bracket expression knows no negation and ends
# at the first ], a backslash escapes the character after it, and a
# malformed pattern such as _x[ matches nothing, without a word. An
# entry whose wildcards are all escaped names a symbol, which must
# exist, as any name the list gives.
cat <<EOF2 | $CC -o $t/c.o -c -xassembler -
.data
.globl "_x[", _xa, "_k!", _ka, _kb, "_k*"
"_x[": .quad 1
_xa: .quad 1
"_k!": .quad 1
_ka: .quad 1
_kb: .quad 1
"_k*": .quad 1
.text
.globl _main
_main: ret
EOF2
printf '_main\n_x[\n_k[!a]\n_k\\*\n' > $t/list3
$CC --ld-path=$mold -o $t/exe3 $t/c.o -Wl,-exported_symbols_list,$t/list3
nm -gU $t/exe3 | awk '{print $3}' | sort | tr '\n' ' ' > $t/syms3
[ "$(cat $t/syms3)" = '_k! _k* _ka _main ' ]
printf '_main\n_nothere\\*\n' > $t/list4
not $CC --ld-path=$mold -o $t/exe4 $t/c.o -Wl,-exported_symbols_list,$t/list4 2> $t/log4
grep -q '_nothere\*' $t/log4
