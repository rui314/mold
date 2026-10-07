#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF2 | $CC -o $t/a.o -c -xc -
int main() {}
EOF2

$CC --ld-path=$mold -o $t/exe1 $t/a.o
otool -l $t/exe1 | grep 'stacksize 0$'

$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-stack_size,200000
otool -l $t/exe2 | grep 'stacksize 2097152$'
$RUN $t/exe2

# The size is checked against the most a stack may take on the target,
# then for a main executable, then for a multiple of the page size. A
# zero size is no size, with a warning.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-stack_size,0 2> $t/log3
grep -q -- '-stack_size 0x0 has no effect' $t/log3
not $CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-stack_size,0x800 2> $t/log4
grep -q -- '-stack_size (0x00000800) must be multiples of page size (0x0000[14]000)' $t/log4
not $CC --ld-path=$mold -o $t/exe5 $t/a.o -Wl,-stack_size,0x10000004000 2> $t/log5
grep -Eq -- '-stack_size must be <= (512MB on arm64 platforms|1TB on x86_64 macOS)' $t/log5
not $CC --ld-path=$mold -shared -o $t/lib.dylib $t/a.o -Wl,-stack_size,0x800 2> $t/log6
grep -q -- '-stack_size option can only be used when linking a main executable' $t/log6
not $CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-stack_size,4k 2> $t/log6
grep -q -- '-stack_size must specify an integer size' $t/log6

# No dyld starts a static executable with LC_MAIN's stack size. Its
# stack is a segment of address space alone, __UNIXSTACK, pinned below a
# fixed top of stack (0x120000000, or 0x7fff5c000000 for x86-64 macOS)
# like a -segaddr, and LC_UNIXTHREAD's stack pointer starts there.
cat <<EOF2 | $CC -o $t/b.o -c -xassembler -
.text
.globl start
start:
  ret
EOF2
$mold -arch $ARCH -static -stack_size 0x8000 $t/b.o -o $t/exe7
otool -l $t/exe7 > $t/lc7
segs() { awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }' $1; }
[ "$(segs $t/lc7)" = '__PAGEZERO __TEXT __UNIXSTACK __LINKEDIT ' ]
grep -A8 'segname __UNIXSTACK' $t/lc7 | awk '{ printf "%s %s ", $1, $2 }' > $t/stack7
if [ $ARCH = arm64 ]; then
  top=0x120000000
  grep -q ' sp  0x0000000120000000 ' $t/lc7
else
  top=0x7fff5c000000
  grep -q ' rsp 0x00007fff5c000000 ' $t/lc7
fi
[ "$(cat $t/stack7)" = "$(printf 'segname __UNIXSTACK vmaddr 0x%016x vmsize 0x%016x fileoff 0 filesize 0 maxprot 0x00000003 initprot 0x00000003 nsects 0 flags 0x0 ' $((top - 0x8000)) 0x8000)" ]
# __LINKEDIT follows __TEXT, far below the stack.
grep -A2 'segname __LINKEDIT' $t/lc7 | grep -q 'vmaddr 0x000000010000[14]000'

# Being pinned, the stack may not lie in __PAGEZERO or overlap another
# segment.
not $mold -arch $ARCH -static -stack_size 0x8000 -pagezero_size 0x800000000000 $t/b.o \
  -o $t/exe8 2> $t/log8
grep -q -- "-segaddr __UNIXSTACK 0x$(printf %X $((top - 0x8000))) conflicts with -pagezero_size" \
  $t/log8
if [ $ARCH = arm64 ]; then
  not $mold -arch $ARCH -static -stack_size 0x20000000 $t/b.o -o $t/exe9 2> $t/log9
  grep -q 'custom segments overlap: __TEXT(0x100000000-0x100004000) __UNIXSTACK(0x100000000-0x120000000)' $t/log9
fi

# -stack_addr moves the top of a static executable's stack. It must be
# a multiple of the page size, come with a size smaller than it, and is
# for static executables only: a dyld-started one's stack is dyld's.
$mold -arch $ARCH -static -stack_size 0x8000 -stack_addr 0x300000000 $t/b.o -o $t/exe10
otool -l $t/exe10 > $t/lc10
grep -A2 'segname __UNIXSTACK' $t/lc10 | grep -q 'vmaddr 0x00000002ffff8000'
grep -Eq ' r?sp +0x0000000300000000 ' $t/lc10
not $mold -arch $ARCH -static -stack_size 0x8000 -stack_addr 0x300000800 $t/b.o \
  -o $t/exe11 2> $t/log11
grep -q -- '-stack_addr (0x300000800) must be multiples of page size (0x0000[14]000)' $t/log11
not $mold -arch $ARCH -static -stack_addr 0x300000000 $t/b.o -o $t/exe12 2> $t/log12
grep -q -- '-stack_addr must be used with -stack_size' $t/log12
not $mold -arch $ARCH -static -stack_size 0x10000 -stack_addr 0x8000 $t/b.o -o $t/exe13 \
  2> $t/log13
grep -q -- '-stack_size (0x00010000) must be smaller than -stack_addr (0x00008000)' $t/log13
not $CC --ld-path=$mold -o $t/exe14 $t/a.o -Wl,-stack_size,0x8000 -Wl,-stack_addr,0x300000000 \
  2> $t/log14
grep -q -- "-stack_addr can't be used with modern executables" $t/log14
$CC --ld-path=$mold -o $t/exe15 $t/a.o -Wl,-stack_addr,0 2> $t/log15
grep -q -- '-stack_addr 0x0 has no effect' $t/log15
