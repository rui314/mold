#!/bin/bash
source "$(dirname "$0")"/common.inc

# Load commands come in ld64's order: segments; a dylib's identity;
# the dyld tables; the symbol tables; the dynamic linker; UUID, build
# and source versions; the entry point; the libraries (command-line
# order, then the auto-linked ones); the run paths; function starts
# and data-in-code; the signature last.
cat <<EOF2 | $CC -o $t/a.o -c -xc -
#include <stdio.h>
int main() { printf("hi\n"); return 0; }
EOF2
cat <<EOF2 | $CC -o $t/b.o -c -xc -
int fn(void) { return 1; }
EOF2
seq() { otool -l $1 | grep '^ *cmd ' | awk '{print $2}' | uniq | tr '\n' ' '; }

$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-rpath,@loader_path -mmacosx-version-min=13.0
[ "$(seq $t/exe)" = "LC_SEGMENT_64 LC_DYLD_CHAINED_FIXUPS LC_DYLD_EXPORTS_TRIE LC_SYMTAB LC_DYSYMTAB LC_LOAD_DYLINKER LC_UUID LC_BUILD_VERSION LC_SOURCE_VERSION LC_MAIN LC_LOAD_DYLIB LC_RPATH LC_FUNCTION_STARTS LC_DATA_IN_CODE LC_CODE_SIGNATURE " ]

$CC --ld-path=$mold -dynamiclib -o $t/libb.dylib $t/b.o -Wl,-rpath,@loader_path -mmacosx-version-min=13.0
[ "$(seq $t/libb.dylib)" = "LC_SEGMENT_64 LC_ID_DYLIB LC_DYLD_CHAINED_FIXUPS LC_DYLD_EXPORTS_TRIE LC_SYMTAB LC_DYSYMTAB LC_UUID LC_BUILD_VERSION LC_SOURCE_VERSION LC_LOAD_DYLIB LC_RPATH LC_FUNCTION_STARTS LC_DATA_IN_CODE LC_CODE_SIGNATURE " ]

$CC --ld-path=$mold -bundle -o $t/b.bundle $t/b.o -Wl,-rpath,@loader_path -mmacosx-version-min=13.0
[ "$(seq $t/b.bundle)" = "LC_SEGMENT_64 LC_DYLD_CHAINED_FIXUPS LC_DYLD_EXPORTS_TRIE LC_SYMTAB LC_DYSYMTAB LC_UUID LC_BUILD_VERSION LC_SOURCE_VERSION LC_LOAD_DYLIB LC_RPATH LC_FUNCTION_STARTS LC_DATA_IN_CODE LC_CODE_SIGNATURE " ]

# Classic dyld info in place of the chained fixups.
$CC --ld-path=$mold -o $t/exe11 $t/a.o -mmacosx-version-min=11.0
[ "$(seq $t/exe11)" = "LC_SEGMENT_64 LC_DYLD_INFO_ONLY LC_SYMTAB LC_DYSYMTAB LC_LOAD_DYLINKER LC_UUID LC_BUILD_VERSION LC_SOURCE_VERSION LC_MAIN LC_LOAD_DYLIB LC_FUNCTION_STARTS LC_DATA_IN_CODE LC_CODE_SIGNATURE " ]
$t/exe | grep -q hi
