#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime rounds the size of a -r output's one segment up to 8 bytes,
# and counts the padding in its file size too: the file size is the
# size less the zero-fill sections' span (from the end of the last
# section with contents), so past a trailing __bss it ends in the
# relocations that follow the contents.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main: ret
.data
.byte 1
EOF
cat <<EOF | $CC -o $t/b.o -c -xassembler -
.text
.globl _main
_main: ret
.data
.byte 1
.zerofill __DATA,__bss,_b,5,4
EOF

# The segment's vmsize and filesize, and the end of its last section
# and of its last one with contents.
sizes() {
  local vm fs end=0 fend=0 a e
  vm=$(otool -l $1 | awk '$1 == "vmsize" { print $2; exit }')
  fs=$(otool -l $1 | awk '$1 == "filesize" { print $2; exit }')
  while read a e flags; do
    e=$((a + e))
    [ $e -gt $end ] && end=$e
    [ $flags != 0x00000001 ] && [ $e -gt $fend ] && fend=$e
  done < <(otool -l $1 | awk '$1 == "addr" { a = $2 } $1 == "size" { s = $2 }
    $1 == "flags" { print a, s, $2 }')
  echo $((vm)) $fs $end $fend
}

$mold -r -arch $ARCH $t/a.o -o $t/a.r.o
read vmsize filesize end fend <<< "$(sizes $t/a.r.o)"
[ $((end % 8)) != 0 ]
[ $vmsize = $(((end + 7) / 8 * 8)) ]
[ $filesize = $vmsize ]

$mold -r -arch $ARCH $t/b.o -o $t/b.r.o
read vmsize filesize end fend <<< "$(sizes $t/b.r.o)"
[ $vmsize = 24 ]
[ $filesize = $((fend + vmsize - end)) ]
