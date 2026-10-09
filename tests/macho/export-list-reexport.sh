#!/bin/bash
source "$(dirname "$0")"/common.inc

# An export list re-exports a dylib's symbol it names: the export trie
# gets a re-export entry (flags EXPORT_SYMBOL_FLAGS_REEXPORT, the
# dylib's ordinal) and the symbol table an N_INDR entry beside the
# import, as for -reexported_symbols_list. A plain name is an initial
# undefine, so it needs no reference from the code, and it keeps its
# dylib under -dead_strip_dylibs; a pattern re-exports what it matches
# among the symbols the link imports anyway (ld-prime).
cat <<EOF | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int foo = 1;
int main() { printf("hi\n"); return 0; }
EOF

printf '_foo\n_zlibVersion\n' > $t/list
$CC --ld-path=$mold -o $t/exe $t/a.o -lz -Wl,-dead_strip_dylibs -Wl,-exported_symbols_list,$t/list
dyld_info -exports $t/exe > $t/exports
grep -q '\[re-export\] _zlibVersion (from libz)' $t/exports
otool -L $t/exe | grep -q libz
nm -m $t/exe | grep -q '(indirect) external _zlibVersion (for _zlibVersion)'
$RUN $t/exe | grep -q hi

$CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -Wl,-exported_symbol,_foo \
  -Wl,-exported_symbol,_printf
dyld_info -exports $t/b.dylib | grep -q '\[re-export\] _printf (from libSystem)'

printf '_foo\n_print*\n_zlib*\n' > $t/list2
$CC --ld-path=$mold -o $t/exe2 $t/a.o -lz -Wl,-exported_symbols_list,$t/list2
dyld_info -exports $t/exe2 > $t/exports2
grep -q '\[re-export\] _printf (from libSystem)' $t/exports2
not grep -q zlib $t/exports2
