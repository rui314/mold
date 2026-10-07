#!/bin/bash
source "$(dirname "$0")"/common.inc

# A section of fixed-size records - literals, pointers to initializers,
# terminators or GOT slots, thread-local variable descriptors, CFString
# constants, Objective-C lists and __LD,__compact_unwind - is split into
# a subsection per record, and an object whose section size is no
# multiple of the record's fails the link, -r too. (A 9-byte
# __mod_init_func used to crash an arm64 link.)
echo 'int main() { return 0; }' | $CC -o $t/main.o -c -xc -

# Checks that the object built from SECTION-DIRECTIVE, whose section
# holds nine bytes, fails a final link and -r.
check() {
  printf '%s\n.p2align 3\n.quad 0\n.byte 0\n.text\n.globl _f\n_f: ret\n' "$1" |
    $CC -o $t/a.o -c -xassembler - &&
    not $CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o 2> /dev/null &&
    not $mold -r -arch $ARCH -o $t/r.o $t/a.o 2> /dev/null
}

check '.section __DATA,__mod_init_func,mod_init_funcs'
check '.section __DATA,__mod_term_func,mod_term_funcs'
check '.section __DATA,__foo,mod_init_funcs'
check '.section __TEXT,__literal4,4byte_literals'
check '.section __TEXT,__literal8,8byte_literals'
check '.section __TEXT,__literal16,16byte_literals'
check '.section __DATA,__thread_vars,thread_local_variables'
check '.section __DATA,__weak_got,non_lazy_symbol_pointers'
check '.section __DATA,__cfstring'
check '.section __DATA,__lazy_load_got'
check '.section __DATA,__objc_selrefs,literal_pointers,no_dead_strip'
check '.section __DATA,__objc_classlist,regular,no_dead_strip'
check '.section __DATA,__objc_catlist,regular,no_dead_strip'
check '.section __DATA,__objc_protorefs,regular,no_dead_strip'
check '.section __LD,__compact_unwind,regular,debug'

# An archive member the link doesn't use is checked too.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.byte 0
.text
.globl _g
_g: ret
EOF
rm -f $t/lib.a
ar rcs $t/lib.a $t/b.o
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/lib.a 2> /dev/null

# Where the name doesn't say what the records are, a section of the
# regular type is not one of them.
cat <<EOF | $CC -o $t/c.o -c -xassembler -
.section __DATA_CONST,__cfstring
.byte 0
.section __DATA_CONST,__objc_classlist
.byte 0
.section __TEXT,__lit8
.byte 0
.text
.globl _h
_h: ret
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/c.o

# A label at the end of such a section names no record: it is ignored,
# in -r too, and a relocation to it, local or global, fails the link.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.section __TEXT,__literal8,8byte_literals
.quad 1
.globl _end8
_end8:
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.quad _main
_endinit:
.subsections_via_symbols
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/d.o
$RUN $t/exe
nm $t/exe > $t/syms
not grep -q '_end8\|_endinit' $t/syms
$mold -r -arch $ARCH -o $t/r.o $t/d.o
nm $t/r.o > $t/syms
not grep -q '_end8\|_endinit' $t/syms

# The ltmpN label the arm64 assembler puts at an empty section's start,
# in an object without subsections, where such labels count, keeps the
# section (but not ld-prime's) and never reaches the symbol table.
if [ $ARCH = arm64 ]; then
  printf '.text\nnop\n.section __TEXT,__literal8,8byte_literals\n' |
    $CC -o $t/e.o -c -xassembler -
  $mold -r -arch arm64 -o $t/r.o $t/e.o
  nm $t/r.o > $t/syms
  not grep -q ltmp1 $t/syms
  if $mold -v 2> /dev/null | grep -q mold-macho; then
    otool -l $t/r.o | grep -q 'sectname __literal8'
  fi
fi

for global in '' '.globl _end8'; do
  printf '%s\n' '.section __TEXT,__literal8,8byte_literals' '.quad 1' "$global" \
    '_end8:' .data '.p2align 3' '.quad _end8' | $CC -o $t/f.o -c -xassembler -
  not $CC --ld-path=$mold -o $t/exe $t/main.o $t/f.o 2> /dev/null
done

# An initializer or terminator pointer needs a relocation to name its
# function.
printf '.section __DATA,__mod_term_func,mod_term_funcs\n.p2align 3\n.quad 0\n' |
  $CC -o $t/g.o -c -xassembler -
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/g.o 2> /dev/null

# An __objc_imageinfo record is 8 bytes: a shorter one is ignored and a
# longer one's first 8 bytes read.
imageinfo() {
  echo '.section __DATA,__objc_imageinfo,regular,no_dead_strip'
  echo '.p2align 2'
  [ $1 -ge 4 ] && echo '.long 0'
  [ $1 -ge 8 ] && echo '.long 64'
  [ $1 -ge 12 ] && echo '.long 7'
  echo '.subsections_via_symbols'
}
for n in 0 4 12; do
  imageinfo $n | $CC -o $t/ii$n.o -c -xassembler -
  $CC --ld-path=$mold -o $t/exe-ii$n $t/main.o $t/ii$n.o 2> /dev/null
  $mold -r -arch $ARCH -o $t/r-ii$n.o $t/ii$n.o 2> /dev/null
done
otool -l $t/r-ii4.o > $t/lc4
not grep -q __objc_imageinfo $t/lc4
otool -s __DATA __objc_imageinfo $t/r-ii12.o > $t/sect12
grep -Eq '^0+	(00000000 00000040|00 00 00 00 40 00 00 00) $' $t/sect12
