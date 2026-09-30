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
# it nothing slides the image, and __mh_execute_header is absolute.
for cmd in LC_DYLD_INFO LC_DYLD_INFO_ONLY LC_DYLD_CHAINED_FIXUPS LC_DYLD_EXPORTS_TRIE LC_DYSYMTAB; do
  not grep -q "cmd $cmd\$" $t/lc
done
nm -m $t/exe > $t/nm
grep -q '(absolute) .*__mh_execute_header' $t/nm

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
