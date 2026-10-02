#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A folded function's name stays in the symbol table, naming the copy
# it folded into: identical local functions of one name in several
# objects (an inline function's .cold.1 part, a static helper) leave it
# there once for each, at one address, as do a private extern of the
# name and those folding into it. (ld-prime lists the survivor's own
# name once of each scope.) -map lists every folded function's name as
# a row of no size at the copy it folded into.
if [ $ARCH = arm64 ]; then
  body() { echo "mov w0, #$1"; echo ret; }
  jump() { echo "b $1"; }
else
  body() { echo "movl \$$1, %eax"; echo ret; echo nop; echo nop; }
  jump() { echo "jmp $1"; }
fi

for i in 1 2 3 4; do
  name=_helper
  [ $i = 3 ] && name=_other
  {
    echo '.subsections_via_symbols'
    echo '.text'
    [ $i = 4 ] && echo ".globl $name" && echo ".private_extern $name"
    echo ".globl _call$i"
    echo ".p2align 2"
    echo "_call$i:"
    jump $name
    echo ".p2align 2"
    echo "$name:"
    body 7
  } > $t/a$i.s
  $CC -o $t/a$i.o -c $t/a$i.s
done

cat <<EOF | $CC -o $t/main.o -c -xc -
int call1(void), call2(void), call3(void);
int main() { return call1() + call2() + call3() != 21; }
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a1.o $t/a2.o $t/a3.o \
  -Wl,-deduplicate -Wl,-map,$t/map
$t/exe
nm $t/exe > $t/nm
[ "$(grep -c ' _helper$' $t/nm)" = 2 ]
[ "$(awk '/ _helper$/ { print $1 }' $t/nm | sort -u | wc -l)" -eq 1 ]
[ "$(grep -c ' _other$' $t/nm)" = 1 ]
grep -E '\] (_helper|_other)$' $t/map > $t/rows
[ "$(wc -l < $t/rows)" -eq 3 ]
[ "$(cut -f1 $t/rows | sort -u | wc -l)" -eq 1 ]
[ "$(cut -f2 $t/rows | grep -vc 0x00000000)" = 1 ]

$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a1.o $t/a2.o $t/a3.o -Wl,-no_deduplicate
[ "$(nm $t/exe2 | grep -c ' _helper$')" = 2 ]

# With the private extern first, the locals fold into it.
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/a4.o $t/a1.o $t/a2.o $t/a3.o -Wl,-deduplicate
$t/exe3
nm -m $t/exe3 | grep ' _helper$' > $t/nm3
[ "$(grep -c . $t/nm3)" = 3 ]
[ "$(awk '{ print $1 }' $t/nm3 | sort -u | wc -l)" -eq 1 ]
grep -q 'non-external (was a private external) _helper$' $t/nm3

# A folded function's other labels, an alternate entry here, go with
# it in -map.
{
  echo '.subsections_via_symbols'
  echo '.text'
  echo '.globl _call5'
  echo '.p2align 2'
  echo '_call5:'
  jump _x5
  echo '.p2align 2'
  echo '_x5:'
  echo '.alt_entry _y5'
  echo '_y5:'
  body 7
} > $t/a5.s
$CC -o $t/a5.o -c $t/a5.s
cat <<EOF | $CC -o $t/main5.o -c -xc -
int call1(void), call5(void);
int main() { return call1() + call5() != 14; }
EOF
$CC --ld-path=$mold -o $t/exe5 $t/main5.o $t/a1.o $t/a5.o -Wl,-deduplicate \
  -Wl,-map,$t/map5
$t/exe5
grep -E '\] (_helper|_x5|_y5)$' $t/map5 > $t/rows5
[ "$(wc -l < $t/rows5)" -eq 3 ]
[ "$(cut -f1 $t/rows5 | sort -u | wc -l)" -eq 1 ]
[ "$(cut -f2 $t/rows5 | grep -vc 0x00000000)" = 1 ]
