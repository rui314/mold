#!/bin/bash
source "$(dirname "$0")"/common.inc

# A -r output keeps the sections of -sectcreate and -add_empty_section:
# each option's contents are named by a local, no-dead-strip symbol
# "l<sect-create>" and the section's name, after the objects' locals in
# command-line order. A section only the options make is byte-aligned
# and regular, ranked as an unknown section of its segment, and comes
# with the file of its first contents - the first option's last; the
# segments only they make follow the inputs' in command-line order.
cat <<EOF | $CC -o $t/a.o -c -xc -
static int s = 1;
int a = 0x11111111;
int *p = &s;
EOF
printf 'AAAA' > $t/d1
printf 'BBBB' > $t/d2

$mold -r -arch $ARCH -o $t/r.o $t/a.o -sectcreate __NEW __a $t/d1 \
  -sectcreate __NEW __b $t/d2 -sectcreate __DATA __data $t/d2 \
  -add_empty_section __OLD __e -sectcreate __DATA __interpose $t/d1

otool -l $t/r.o | awk '$1 == "sectname" { s = $2 } $1 == "segname" { print $2 "," s }' |
  tail -n +2 | tr '\n' ' ' > $t/log
grep -q '__DATA,__data __DATA,__interpose __NEW,__b __NEW,__a __OLD,__e ' $t/log
otool -l $t/r.o | grep -A5 'sectname __a$' > $t/log2
grep -q 'align 2^0 (1)' $t/log2
otool -X -s __NEW __a $t/r.o | cut -f2 | tr -d ' \n' > $t/log3
grep -qx 41414141 $t/log3
otool -X -s __DATA __data $t/r.o | cut -f2 | tr -d ' \n' > $t/log4
grep -q '^11111111.*42424242$' $t/log4

nm -ap $t/r.o > $t/log5
grep -A6 ' d _s$' $t/log5 | cut -c18- | tr '\n' ' ' > $t/log6
grep -q "^d _s s l<sect-create>__NEW,__a s l<sect-create>__NEW,__b d l<sect-create>__DATA,__data s l<sect-create>__OLD,__e s l<sect-create>__DATA,__interpose " $t/log6
nm -m $t/r.o | grep -q '(__NEW,__a) non-external \[no dead strip\] l<sect-create>__NEW,__a$'

# A final link keeps the section, live under -dead_strip.
cat <<EOF | $CC -o $t/main.o -c -xc -
int main() { return 0; }
EOF
$CC --ld-path=$mold -o $t/exe $t/main.o $t/r.o -Wl,-dead_strip
otool -X -s __NEW __a $t/exe | cut -f2 | tr -d ' \n' > $t/log7
grep -qx 41414141 $t/log7
