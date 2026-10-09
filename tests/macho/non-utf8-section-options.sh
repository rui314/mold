#!/bin/bash
source "$(dirname "$0")"/common.inc

# The options that name a segment or a section take any bytes, UTF-8
# or not, as ld-prime does: they reach the output's headers as given
# (cut to the 16 bytes of a name field, inside a UTF-8 character too),
# and the diagnostics print them with a U+FFFD for each byte that isn't
# UTF-8.
cat <<EOF | $CC -o $t/a.o -c -xc -
long x = 42;
int main() { return x != 42; }
EOF
echo hello > $t/data

seg=$'__D\xffT'
sect=$'__s\xfeC'
shown_seg=$'__D\xef\xbf\xbdT'
shown_sect=$'__s\xef\xbf\xbdC'
link="$CC --ld-path=$mold $t/a.o"

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { print $2 "," s; s = "" }'
}
segs() { otool -l $1 | awk '$1 == "segname" && !seen[$2]++ { print $2 }'; }
# A segment's protections and address.
prot() {
  otool -l $1 | awk -v s="$2" '$1 == "segname" { seg = $2 }
    seg == s && $1 == "maxprot" { m = $2 } seg == s && $1 == "initprot" { print m + 0, $2 + 0; exit }'
}
vmaddr() { otool -l $1 | awk -v s="$2" '$1 == "segname" { seg = $2 } seg == s && $1 == "vmaddr" { print $2; exit }'; }

# -sectcreate and -add_empty_section. A -r output names the contents
# with a symbol of the names too.
$link -o $t/exe -Wl,-sectcreate,"$seg","$sect",$t/data
$RUN $t/exe
sects $t/exe | grep -aqx "$seg,$sect"
$link -o $t/exe2 -Wl,-add_empty_section,"$seg","$sect"
sects $t/exe2 | grep -aqx "$seg,$sect"
$mold -r -arch $ARCH -o $t/r.o $t/a.o -sectcreate "$seg" "$sect" $t/data
sects $t/r.o | grep -aqx "$seg,$sect"
nm -ap $t/r.o | grep -aq " l<sect-create>$seg,$sect$"

# A name longer than 16 bytes is cut, here inside the é, with a warning
# that prints both names.
long=$'__AAAAAAAAAAAAA\xc3\xa9X'
cut=$'__AAAAAAAAAAAAA\xc3'
shown_cut=$'__AAAAAAAAAAAAA\xef\xbf\xbd'
$link -o $t/exe3 -Wl,-sectcreate,"$long",__s,$t/data 2> $t/log3
grep -aqF "-sectcreate segment name too long ('$long'), will be truncated to '$shown_cut'" $t/log3
segs $t/exe3 | grep -aqx "$cut"

# -segprot, -segaddr and -sectalign.
$link -o $t/exe4 -Wl,-sectcreate,"$seg","$sect",$t/data -Wl,-segprot,"$seg",r,r \
  -Wl,-segaddr,"$seg",0x300000000 -Wl,-sectalign,"$seg","$sect",0x100
[ "$(prot $t/exe4 "$seg")" = '1 1' ]
[ "$(vmaddr $t/exe4 "$seg")" = 0x0000000300000000 ]
otool -l $t/exe4 | grep -aA8 "sectname $sect" | grep -q 'align 2^8 (256)'

$link -o $t/exe5 -Wl,-sectcreate,"$seg","$sect",$t/data -Wl,-sectalign,"$seg","$sect",0x30 \
  -Wl,-segaddr,"$seg",0x300000000 -Wl,-segaddr,"$seg",0x300000000 2> $t/log5
grep -aqF "alignment for -sectalign $shown_seg $shown_sect is not a power of two, using 0x10" $t/log5
grep -aqF -- "-segaddr $shown_seg used more than once" $t/log5

# -rename_section and -rename_segment, from and to such names.
$link -o $t/exe6 -Wl,-rename_section,__DATA,__data,"$seg","$sect"
$RUN $t/exe6
sects $t/exe6 | grep -aqx "$seg,$sect"
$link -o $t/exe7 -Wl,-sectcreate,"$seg","$sect",$t/data -Wl,-rename_segment,"$seg",$'__N\xf0W'
sects $t/exe7 | grep -aqx $'__N\xf0W'",$sect"

# -move_to_rw_segment, and its warning about code, which names the
# segment as given.
echo _x > $t/list
$link -o $t/exe8 -Wl,-move_to_rw_segment,"$seg",$t/list
$RUN $t/exe8
sects $t/exe8 | grep -aqx "$seg,__data"
echo _main > $t/list2
$link -o $t/exe9 -Wl,-move_to_rw_segment,"$seg",$t/list2 2> $t/log9
grep -aqF "to segment '$shown_seg' because" $t/log9

# -segment_order, -section_order and -seg_page_size, in a -static
# image.
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl __start
__start:
  ret
.data
.quad 1
EOF
static="$mold -arch $ARCH -static -e __start $t/b.o -sectcreate $seg $sect $t/data"
$static -o $t/exe10 -segment_order "$seg:__DATA" -section_order "$seg" "$sect" \
  -seg_page_size "$seg" 0x8000 2> $t/log10
[ "$(segs $t/exe10 | tr '\n' ' ')" = "__PAGEZERO __TEXT $seg __DATA __LINKEDIT " ]
not $static -o $t/exe11 -segment_order "$seg" 2> $t/log11
not $static -o $t/exe12 -section_order "$seg" __a -section_order "$seg" __b 2> $t/log12
grep -aqF -- "-section_order $shown_seg used more than once" $t/log12
