#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -static image (a kernel, a boot loader) runs without dyld, so it
# has no LC_MAIN for dyld to read: its thread starts from LC_UNIXTHREAD,
# a register state that is all zero but the program counter, which
# holds the entry point. macOS still runs such an x86-64 program; an
# arm64 one it refuses, so there only the load commands are checked.
if [ $ARCH = arm64 ]; then
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl __start
.p2align 2
__start:
  mov x0, #42
  mov x16, #1
  svc #0x80
EOF
else
  cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl __start
__start:
  movl \$42, %edi
  movl \$0x2000001, %eax
  syscall
EOF
fi

cat <<EOF | $CC -o $t/b.o -c -xassembler -
.data
.globl _p
.p2align 3
_p: .quad __start
EOF

$mold -arch $ARCH -static -e __start $t/a.o $t/b.o -o $t/exe
otool -l $t/exe > $t/lc
grep -q 'cmd LC_UNIXTHREAD' $t/lc
not grep -q 'cmd LC_MAIN' $t/lc
not grep -q 'cmd LC_LOAD_DYLINKER' $t/lc
entry=$(nm $t/exe | awk '$3 == "__start" { print $1 }')
if [ $ARCH = arm64 ]; then
  grep -q " pc 0x$entry" $t/lc
else
  grep -q "rip 0x$entry" $t/lc
  rc=0
  $t/exe || rc=$?
  [ $rc = 42 ]
fi

# No dyld reads the image either: it has no fixups, binds or exports,
# and a pointer holds its final address. Only -pie keeps a dynamic
# symbol table, for the local relocations that slide the image; without
# it nothing slides the image, and __mh_execute_header is absolute
# (N_ABS), in no section. (ld-prime keeps its section number, 1.)
for cmd in LC_DYLD_INFO LC_DYLD_INFO_ONLY LC_DYLD_CHAINED_FIXUPS LC_DYLD_EXPORTS_TRIE LC_DYSYMTAB; do
  not grep -q "cmd $cmd\$" $t/lc
done
nm -m $t/exe > $t/nm
grep -q '(absolute) .*__mh_execute_header' $t/nm
nm -x $t/exe | grep -Eq '^[0-9a-f]+ 03 00 0010 [0-9a-f]+ __mh_execute_header$'

$mold -arch $ARCH -static -pie -e __start $t/a.o $t/b.o -o $t/exe2
otool -l $t/exe2 > $t/lc2
grep -q 'cmd LC_DYSYMTAB$' $t/lc2
nm -m $t/exe2 > $t/nm2
grep -q '(__TEXT,__text) .*__mh_execute_header' $t/nm2

# Its local relocations stand in for dyld's rebase info: one non-extern
# 8-byte UNSIGNED relocation per pointer, r_symbolnum naming the section
# it points into (__text), its address counted from the first segment
# (on x86-64 the first writable one).
otool -r $t/exe2 > $t/rel2
grep -q 'Local relocation information 1 entries' $t/rel2
if [ $ARCH = arm64 ]; then seg=__TEXT; else seg=__DATA; fi
base=$(otool -l $t/exe2 | awk -v s=$seg '$1 == "segname" && $2 == s { getline; print $2; exit }')
p=$(nm $t/exe2 | awk '$3 == "_p" { print $1 }')
rel=$(printf '%08x 0 3 0 0 0 1' $((0x$p - base)))
[ "$(awk '$1 ~ /^[0-9a-f]+$/ { print $1, $2, $3, $4, $5, $6, $7 }' $t/rel2)" = "$rel" ]

# The code tables and the build version are left out unless asked for;
# asked for, they go where they would in any image.
for cmd in LC_FUNCTION_STARTS LC_DATA_IN_CODE LC_BUILD_VERSION; do
  not grep -q "cmd $cmd\$" $t/lc
done
$mold -arch $ARCH -static -e __start -function_starts -data_in_code_info \
  -version_load_command $t/a.o $t/b.o -o $t/exe3
otool -l $t/exe3 | awk '$1 == "cmd" { printf "%s ", $2 }' > $t/cmds3
grep -q 'LC_UUID LC_BUILD_VERSION LC_SOURCE_VERSION LC_UNIXTHREAD LC_FUNCTION_STARTS LC_DATA_IN_CODE' $t/cmds3

# Nor is it ad-hoc signed, even on arm64, unless -adhoc_codesign says so.
not grep -q 'cmd LC_CODE_SIGNATURE$' $t/lc
$mold -arch $ARCH -static -e __start -adhoc_codesign $t/a.o $t/b.o -o $t/exe4
otool -l $t/exe4 | grep -q 'cmd LC_CODE_SIGNATURE$'

# It has no __unwind_info either: its compact unwind records go, and an
# x86-64 image keeps every FDE of its objects' __eh_frame instead.
cat <<EOF | $CC -o $t/c.o -c -xc -O1 -
int g(int);
int f(int x) { return g(x) + 1; }
int g(int x) { return x * 2; }
EOF
$mold -arch $ARCH -static -e __start $t/a.o $t/b.o $t/c.o -o $t/exe5
otool -l $t/exe5 > $t/lc5
not grep -q 'sectname __unwind_info' $t/lc5
if [ $ARCH = x86_64 ]; then
  size() { otool -l $1 | grep -A4 'sectname __eh_frame' | awk '$1 == "size" { print $2 }'; }
  [ "$(size $t/exe5)" = "$(size $t/c.o)" ]
fi

# Nor __DATA_CONST, a segment dyld makes read-only once it has fixed it
# up: constant data stays in __DATA. -data_const asks for the segment,
# which then comes after __DATA.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.section __DATA,__const
.p2align 3
_cp: .quad __start
EOF
segs() { otool -l $1 | awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }'; }
$mold -arch $ARCH -static -e __start $t/a.o $t/b.o $t/d.o -o $t/exe6
[ "$(segs $t/exe6)" = '__PAGEZERO __TEXT __DATA __LINKEDIT ' ]
$mold -arch $ARCH -static -e __start -data_const $t/a.o $t/b.o $t/d.o -o $t/exe7
[ "$(segs $t/exe7)" = '__PAGEZERO __TEXT __DATA __DATA_CONST __LINKEDIT ' ]

# No Objective-C runtime sets up an image no dyld loads: ld-prime gives
# it no __objc_imageinfo, as it does a kext.
cat <<EOF | $CC -o $t/e.o -c -xassembler -
.section __DATA,__objc_imageinfo,regular,no_dead_strip
.long 0, 64
EOF
$mold -arch $ARCH -static -e __start $t/a.o $t/b.o $t/e.o -o $t/exe8
otool -l $t/exe8 > $t/lc8
not grep -q __objc_imageinfo $t/lc8
$mold -arch $ARCH -preload -e __start $t/a.o $t/b.o $t/e.o -o $t/exe9
otool -l $t/exe9 > $t/lc9
not grep -q __objc_imageinfo $t/lc9
