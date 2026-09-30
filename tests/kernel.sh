#!/bin/bash
source "$(dirname "$0")"/common.inc

# -kernel marks a -static image as a kernel, which kmutil slides into
# a kernel collection (XNU links itself with -static -pie -kernel
# -add_split_seg_info): ld-prime makes it position independent and
# gives it split info and __DATA_CONST, as it does a dylib bound for
# the dyld shared cache. It must be -static.
cat <<EOF | $CC -o $t/a.o -c -xc -O1 -
int counter = 1;
int *cptr = &counter;
static const char *names[] = { "a", "b" };
const char *get(int i) { return names[i & 1]; }
void _start(void) { counter = *cptr + (int)(long)get(counter); }
EOF
# A function with DWARF unwind info: the kernel keeps its FDE.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _dw
.p2align 2
_dw:
  .cfi_startproc
  .cfi_escape 0x2e, 0x10
  ret
  .cfi_endproc
.subsections_via_symbols
EOF

if [ $ARCH = arm64 ]; then
  base=0xfffffff007004000
  exec_text=(-rename_section __TEXT __text __TEXT_EXEC __text)
else
  base=0xffffff8000200000
  exec_text=()
fi
$mold -arch $ARCH -static -kernel -e __start -pagezero_size 0 -image_base $base \
  -function_starts "${exec_text[@]}" $t/a.o $t/b.o -o $t/exe
otool -hv $t/exe | grep -q PIE
otool -l $t/exe > $t/lc
otool -l $t/exe | awk '$1 == "cmd" { printf "%s ", $2 }' > $t/cmds
grep -q 'LC_UNIXTHREAD LC_SEGMENT_SPLIT_INFO' $t/cmds
grep -q 'cmd LC_DYSYMTAB$' $t/lc
grep -A1 'sectname __const' $t/lc | grep -q 'segname __DATA_CONST'

# Code renamed into __TEXT_EXEC runs from there (r-x), and leaves no
# empty __TEXT,__text behind.
if [ $ARCH = arm64 ]; then
  grep -A8 'segname __TEXT_EXEC' $t/lc | grep -q 'maxprot 0x00000005'
  grep -A1 'sectname __text' $t/lc > $t/text
  not grep -q 'segname __TEXT$' $t/text
fi

# Function starts count from the image base.
start=$(nm $t/exe | awk '$3 == "__start" { print $1 }')
dyld_info -function_starts $t/exe | grep -qi "0x$start *__start"

# The split info: data pointers, and on x86-64 the FDE's CIE pointer
# (recorded against the FDE) and function.
dyld_info -shared_region $t/exe > $t/split
grep -Eq '__DATA +__data .* __DATA +__data .* 2$' $t/split
if [ $ARCH = x86_64 ]; then
  grep -Eq '__TEXT +__eh_frame .* __TEXT +__text .* 4$' $t/split
  grep -Eq '__TEXT +__eh_frame .* __TEXT +__eh_frame .* 3$' $t/split
fi

$mold -arch $ARCH -static -kernel -e __start -not_for_dyld_shared_cache $t/a.o -o $t/exe2
otool -l $t/exe2 > $t/lc2
not grep -q 'cmd LC_SEGMENT_SPLIT_INFO$' $t/lc2

not $mold -arch $ARCH -kernel -e __start $t/a.o -o $t/exe3 2> $t/log
grep -q -- '-kernel must be used with -static' $t/log

# A kernel has no __PAGEZERO, PIE or not, unless -pagezero_size asks
# for one.
segs() { otool -l $1 | awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }'; }
$mold -arch $ARCH -static -kernel -e __start $t/a.o -o $t/exe4
[ "$(segs $t/exe4)" = '__TEXT __DATA __DATA_CONST __LINKEDIT ' ]
$mold -arch $ARCH -static -kernel -e __start -no_pie $t/a.o -o $t/exe5
[ "$(segs $t/exe5)" = '__TEXT __DATA __DATA_CONST __LINKEDIT ' ]
$mold -arch $ARCH -static -kernel -e __start -pagezero_size 0x4000 $t/a.o -o $t/exe6
[ "$(segs $t/exe6)" = '__PAGEZERO __TEXT __DATA __DATA_CONST __LINKEDIT ' ]
