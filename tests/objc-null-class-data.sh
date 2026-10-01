#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime refuses an object whose __objc_classlist names a class of it
# whose data field, 32 bytes in, has no relocation, whatever its bytes,
# as it reads the object: in any link, and for an archive member it
# doesn't load too. It names the file by the path it was given.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.section __DATA,__objc_data
.globl _OBJC_CLASS_\$_Foo
.p2align 3
_OBJC_CLASS_\$_Foo:
.quad 0, 0, 0, 0, 0x1234
.section __DATA,__objc_classlist,regular,no_dead_strip
.p2align 3
.quad _OBJC_CLASS_\$_Foo
.text
.globl _foo
_foo: ret
.subsections_via_symbols
EOF
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o 2> $t/log
grep -F "null objc class data for '_OBJC_CLASS_\$_Foo' in '$t/a.o'" $t/log

not $mold -r -arch $ARCH -o $t/r.o $t/a.o 2> $t/log
grep -F "null objc class data for '_OBJC_CLASS_\$_Foo' in '$t/a.o'" $t/log

rm -f $t/liba.a
ar rcs $t/liba.a $t/a.o
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/liba.a 2> $t/log
grep -F "null objc class data for '_OBJC_CLASS_\$_Foo' in '$t/liba.a(a.o)'" $t/log
