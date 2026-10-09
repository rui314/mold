#!/bin/bash
source "$(dirname "$0")"/common.inc

# A section header's reloff points at its relocation entries, and a
# section with none has reloff 0 in ld-prime's -r output - also the
# sections a -r link writes afresh, such as x86-64's __eh_frame. The
# entries follow the section contents, section by section in the
# sections' order, the written-afresh ones among the others.
cat <<EOF2 | $CXX -o $t/a.o -c -xc++ -
extern const char msg[] = "hi";
int data = 3;
int *ptr = &data;
void g();
int f() { try { g(); } catch (int) { return 1; } return data; }
EOF2

$mold -arch $ARCH -r $t/a.o -o $t/r.o
otool -l $t/r.o | awk '$1 == "sectname" { s = $2 } $1 == "reloff" { r = $2 }
  $1 == "nreloc" { print s, r, $2 }' > $t/relocs
not grep -Eq ' [1-9][0-9]* 0$' $t/relocs
grep -Eq '^__text [1-9][0-9]* [1-9]' $t/relocs
grep -Eq '^__data [1-9][0-9]* [1-9]' $t/relocs
awk '$3 > 0 { if (end && $2 != end) exit 1; end = $2 + $3 * 8 }' $t/relocs
