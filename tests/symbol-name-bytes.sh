#!/bin/bash
source "$(dirname "$0")"/common.inc

# A symbol name is bytes, any but NUL, UTF-8 or not: ld-prime resolves,
# exports and imports one as it is, writes it to the symbol table, the
# export trie, the binds and -map as it came, and prints it byte for
# byte in a diagnostic.

cat <<'EOF' | $CC -o $t/a.o -c -xc -
int foo(void) __asm__("_f\377o");
int foo(void) { return 3; }
EOF

cat <<'EOF' | $CC -o $t/main.o -c -xc -
int foo(void) __asm__("_f\377o");
int main() { return foo(); }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -Wl,-map,$t/map
$RUN $t/exe || [ $? = 3 ]
nm $t/exe > $t/nm
grep -q $' T _f\xffo$' $t/nm
grep -q $'] _f\xffo$' $t/map

# A dylib exports it, and a client binds to it by that name.
$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/a.o -Wl,-install_name,@rpath/libfoo.dylib
nm -g $t/libfoo.dylib > $t/nm2
grep -q $' T _f\xffo$' $t/nm2
$CC --ld-path=$mold -o $t/exe2 $t/main.o -L$t -lfoo -Wl,-rpath,$t
nm -m $t/exe2 > $t/nm3
grep -q $'(undefined) external _f\xffo (from libfoo)' $t/nm3
$RUN $t/exe2 || [ $? = 3 ]

# An undefined symbol and a duplicate one are named as they are.
not $CC --ld-path=$mold -o $t/exe3 $t/main.o 2> $t/log3
grep -q $'_f\xffo' $t/log3
not $CC --ld-path=$mold -o $t/exe4 $t/main.o $t/a.o $t/a.o 2> $t/log4
grep -q $'duplicate symbol \'_f\xffo\' in:' $t/log4
