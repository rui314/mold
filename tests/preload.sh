#!/bin/bash
source "$(dirname "$0")"/common.inc

# -preload makes an MH_PRELOAD image for firmware: a loader copies its
# segments into ROM or RAM and jumps to the entry point, so the mach
# header, the load commands and the symbol table lie outside every
# segment. There is no __PAGEZERO and no __LINKEDIT segment: the header
# fills the file's first 4 KiB page, the segments follow on 4 KiB pages
# (on arm64 too) from address 0, and the symbol table follows them.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _start
.p2align 2
_start:
  ret
.data
.globl _p
.p2align 3
_p: .quad _q
_q: .quad 0
EOF

$mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe
otool -hv $t/exe > $t/hdr
grep -q ' PRELOAD .* NOUNDEFS$' $t/hdr
otool -l $t/exe > $t/lc
cmds() { awk '$1 == "cmd" { printf "%s ", $2 }' $1; }
[ "$(cmds $t/lc)" = 'LC_SEGMENT_64 LC_SEGMENT_64 LC_SYMTAB LC_UUID LC_UNIXTHREAD ' ]
segs() { awk '$1 == "segname" && !seen[$2]++ { printf "%s ", $2 }' $1; }
[ "$(segs $t/lc)" = '__TEXT __DATA ' ]

# Each segment's vmaddr, fileoff and filesize.
seg() { awk -v s=$2 '$1 == "segname" && $2 == s { getline; a = $2; getline; getline; o = $2; getline; print a, o, $2; exit }' $1; }
[ "$(seg $t/lc __TEXT)" = '0x0000000000000000 4096 4096' ]
[ "$(seg $t/lc __DATA)" = '0x0000000000001000 8192 4096' ]
grep -q 'symoff 12288$' $t/lc

# The pointer holds its final address: nothing relocates the image.
# No __mh_execute_header names the header, which is in no segment.
nm $t/exe > $t/nm
grep -q '^0000000000000000 T _start$' $t/nm
[ "$(od -An -tx1 -j 8192 -N 8 $t/exe | tr -d ' \n')" = 0810000000000000 ]
not grep -q __mh_execute_header $t/nm
if [ $ARCH = arm64 ]; then
  grep -q ' pc 0x0000000000000000' $t/lc
else
  grep -q 'rip 0x0000000000000000' $t/lc
fi

# The entry point is "start" unless -e says otherwise.
not $mold -arch $ARCH -preload $t/a.o -o $t/exe2 2> $t/log2
grep -q '"*start' $t/log2

# -pie makes the image slidable by its own loader: LC_DYSYMTAB lists
# the pointers as local relocations, which follow the segments.
$mold -arch $ARCH -preload -pie -e _start $t/a.o -o $t/exe3
otool -hv $t/exe3 | grep -q ' PRELOAD .* NOUNDEFS PIE$'
otool -l $t/exe3 > $t/lc3
[ "$(cmds $t/lc3)" = 'LC_SEGMENT_64 LC_SEGMENT_64 LC_SYMTAB LC_DYSYMTAB LC_UUID LC_UNIXTHREAD ' ]
grep -q 'locreloff 12288$' $t/lc3
otool -r $t/exe3 | grep -q 'Local relocation information 1 entries'

# Nothing outside the segments but the symbol table: ld-prime ignores
# the options asking for the code tables, a build version, a signature
# or run paths, and sets no MH_APP_EXTENSION_SAFE.
$mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe4 -function_starts \
  -data_in_code_info -version_load_command -adhoc_codesign -rpath /foo \
  -application_extension
otool -l $t/exe4 > $t/lc4
[ "$(cmds $t/lc4)" = 'LC_SEGMENT_64 LC_SEGMENT_64 LC_SYMTAB LC_UUID LC_UNIXTHREAD ' ]
otool -hv $t/exe4 | grep -q ' PRELOAD .* NOUNDEFS$'

# -segment_order may put __TEXT anywhere: it holds no header here.
$mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe5 -segment_order __DATA:__TEXT 2> $t/log5
not grep -q warning $t/log5
otool -l $t/exe5 > $t/lc5
[ "$(segs $t/lc5)" = '__DATA __TEXT ' ]
[ "$(seg $t/lc5 __DATA)" = '0x0000000000000000 4096 4096' ]
[ "$(seg $t/lc5 __TEXT)" = '0x0000000000001000 8192 4096' ]

# The header takes as many pages as its load commands need, with no
# -headerpad: a 32-byte header and 4064 bytes of commands fill a page.
# The commands are 3 fixed ones (336 bytes on arm64, 232 on x86-64), a
# 72-byte LC_SEGMENT_64 per segment and 80 bytes per section.
if [ $ARCH = arm64 ]; then nsegs=2 nsects=40; else nsegs=9 nsects=28; fi
{
  echo '.globl _start'
  echo '.text'
  echo '_start: ret'
  for i in $(seq 1 $nsects); do echo ".section __DATA,__d$i"; echo '.quad 0'; done
  for i in $(seq 1 $nsegs); do echo ".section __SEG$i,__s"; echo '.quad 0'; done
} | $CC -o $t/b.o -c -xassembler -
$mold -arch $ARCH -preload -e _start $t/b.o -o $t/exe6 -headerpad 0x1000
otool -h $t/exe6 | grep -q ' 4064 '
otool -l $t/exe6 > $t/lc6
[ "$(seg $t/lc6 __TEXT | cut -d' ' -f2)" = 4096 ]

echo '.section __DATA,__d0' > $t/c.s
echo '.quad 0' >> $t/c.s
$CC -o $t/c.o -c $t/c.s
$mold -arch $ARCH -preload -e _start $t/b.o $t/c.o -o $t/exe7
otool -l $t/exe7 > $t/lc7
[ "$(seg $t/lc7 __TEXT | cut -d' ' -f2)" = 8192 ]

# A section may be aligned beyond a page; its segment then starts on
# the alignment, and the file skips as much as memory does.
cat <<EOF | $CC -o $t/d.o -c -xassembler -
.globl _start
.text
_start: ret
.data
.p2align 14
.quad 1
EOF
$mold -arch $ARCH -preload -e _start $t/d.o -o $t/exe8
otool -l $t/exe8 > $t/lc8
[ "$(seg $t/lc8 __DATA)" = '0x0000000000004000 20480 4096' ]

# A main executable's options are refused.
not $mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe9 -pagezero_size 0x1000 2> $t/log9
grep -q -- '-pagezero_size can only be used when linking a main executable' $t/log9
not $mold -arch $ARCH -preload -e _start $t/a.o -o $t/exe9 -stack_size 0x10000 2> $t/log9
grep -q -- '-stack_size option can only be used when linking a main executable' $t/log9

# The last of -static and -preload names the output type.
$mold -arch $ARCH -static -preload -e _start $t/a.o -o $t/exe10
otool -hv $t/exe10 | grep -q ' PRELOAD '
$mold -arch $ARCH -preload -static -e _start $t/a.o -o $t/exe11
otool -hv $t/exe11 | grep -q ' EXECUTE '

# -dead_strip keeps what the entry point reaches: -export_dynamic, which
# makes an executable's globals roots, does not keep a -preload image's.
cat <<EOF2 | $CC -o $t/e.o -c -xassembler -
.text
.globl _start, _unused
_start: ret
_unused: ret
.subsections_via_symbols
EOF2
$mold -arch $ARCH -preload -e _start $t/e.o -o $t/exe12 -dead_strip -export_dynamic
nm $t/exe12 > $t/nm12
grep -q _start $t/nm12
not grep -q _unused $t/nm12

# -fixup_chains makes it PIE with chained pointers, their table after
# the segments, but ld-prime writes no command that names the table.
$mold -arch $ARCH -preload -fixup_chains -e _start $t/a.o -o $t/exe13
otool -hv $t/exe13 | grep -q ' PRELOAD .* NOUNDEFS PIE$'
otool -l $t/exe13 > $t/lc13
[ "$(cmds $t/lc13)" = 'LC_SEGMENT_64 LC_SEGMENT_64 LC_SYMTAB LC_DYSYMTAB LC_UUID LC_UNIXTHREAD ' ]
grep -q 'nlocrel 0$' $t/lc13
not grep -q 'symoff 12288$' $t/lc13
