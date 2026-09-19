#!/bin/bash
source "$(dirname "$0")"/common.inc

# -ObjC loads every archive member that carries Objective-C class or
# category metadata - and, in ld64, any member with a __TEXT,__swift*
# section: a Swift type registers with the runtime through its type
# descriptors even without an Objective-C class list, and an archive
# member nobody references by symbol is only linked this way (iTerm2's
# test bundle expects the app's debug dylib, linked with -ObjC against
# libiTerm2SharedARC.a, to export every Swift symbol in the archive).
# An __objc_imageinfo alone does not qualify.
mk() {
  printf 'int fn_%s(void) { return 1; }\n__attribute__((section("%s"),used)) static const char data_%s[8] = "x";\n' \
    $1 "$2" $1 | $CC -o $t/$1.o -c -xc -
}
mk types '__TEXT,__swift5_types'
mk proto '__TEXT,__swift5_proto'
mk modhash '__TEXT,__swift_modhash'
mk imageinfo '__DATA,__objc_imageinfo'
mk selrefs '__DATA,__objc_selrefs'
printf 'int fn_plain(void) { return 1; }\n' | $CC -o $t/plain.o -c -xc -
rm -f $t/lib.a
ar rcs $t/lib.a $t/types.o $t/proto.o $t/modhash.o $t/imageinfo.o $t/selrefs.o $t/plain.o

printf 'int main() { return 0; }\n' | $CC -o $t/main.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a -Wl,-ObjC
nm $t/exe > $t/nm
grep -q ' T _fn_types$' $t/nm
grep -q ' T _fn_proto$' $t/nm
grep -q ' T _fn_modhash$' $t/nm
not grep -q '_fn_imageinfo' $t/nm
not grep -q '_fn_selrefs' $t/nm
not grep -q '_fn_plain' $t/nm

# Without -ObjC nothing is pulled in.
$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/lib.a
nm $t/exe2 > $t/nm2
not grep -q '_fn_' $t/nm2
