#!/bin/bash
source "$(dirname "$0")"/common.inc

# -seg_page_size SEG SIZE makes the segment after SEG start on a SIZE
# boundary, counted from SEG's start, in memory and in the file; SEG's
# own size stays page-rounded, but for __LINKEDIT's. XNU's x86-64 kernel
# starts the segment after __TEXT on a 2 MiB boundary that way.
cat <<EOF | $CC -o $t/a.o -c -xc -
int data = 5;
static int bss[0x4000];
int main() { return data + bss[3] - 5; }
EOF

seg() { awk -v s=$2 '$1 == "segname" && $2 == s { getline; a = $2; getline; v = $2; getline; o = $2; getline; print a, v, o, $2; exit }' $1; }

$CC --ld-path=$mold -o $t/exe1 $t/a.o -Wl,-seg_page_size,__TEXT,0x200000
otool -l $t/exe1 > $t/lc1
read text_addr text_size text_off text_filesize <<< "$(seg $t/lc1 __TEXT)"
read data_addr data_size data_off data_filesize <<< "$(seg $t/lc1 __DATA)"
[ $((data_addr)) = $((text_addr + 0x200000)) ]
[ $data_off = $((0x200000)) ]
[ $text_filesize = $((text_size)) ]
[ $((text_size)) -lt $((0x200000)) ]
$t/exe1

# The boundary counts from the segment's start: the next one goes at
# its start plus its size rounded up to the boundary.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -Wl,-seg_page_size,__DATA,0x100000
otool -l $t/exe2 > $t/lc2
read data_addr data_size data_off data_filesize <<< "$(seg $t/lc2 __DATA)"
read le_addr le_size le_off le_filesize <<< "$(seg $t/lc2 __LINKEDIT)"
[ $((le_addr)) = $((data_addr + 0x100000)) ]
[ $le_off = $((data_off + 0x100000)) ]
$t/exe2

# __LINKEDIT, which no segment follows, takes the boundary as its size.
# The first size given for a segment wins.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -Wl,-seg_page_size,__LINKEDIT,0x100000 \
  -Wl,-seg_page_size,__LINKEDIT,0x200000
otool -l $t/exe3 > $t/lc3
[ "$(seg $t/lc3 __LINKEDIT | awk '{ print $2 }')" = 0x0000000000100000 ]
$t/exe3

# A size that is no power of two rounds down to one; one below the
# page size is an error.
$CC --ld-path=$mold -o $t/exe4 $t/a.o -Wl,-seg_page_size,__TEXT,0x201000 2> $t/log4
grep -q -- '-seg_page_size for __TEXT is not a power of two, rounding down to 0x200000' $t/log4
otool -l $t/exe4 > $t/lc4
[ "$(seg $t/lc1 __DATA)" = "$(seg $t/lc4 __DATA)" ]
not $CC --ld-path=$mold -o $t/exe5 $t/a.o -Wl,-seg_page_size,__TEXT,0x800 2> $t/log5
grep -q -- "-seg_page_size __TEXT 0x800 can't be smaller than page size (0x[14]000)" $t/log5
not $CC --ld-path=$mold -o $t/exe6 $t/a.o -Wl,-seg_page_size,__TEXT,2m 2> $t/log6
grep -q -- '-seg_page_size: not a hexadecimal number: 2m' $t/log6
not $mold -arch $ARCH -o $t/exe7 $t/a.o -seg_page_size __TEXT 2> $t/log7
grep -q -- '-seg_page_size' $t/log7
