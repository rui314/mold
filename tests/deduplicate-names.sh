#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A folded function's name stays in the symbol table, naming the copy
# it folded into, but of that copy's own name ld-prime lists one of each
# scope: identical local functions of one name in several objects (an
# inline function's .cold.1 part, a static helper) leave it there once,
# and once besides a private extern of the name. In -map the names that
# stay are the linker's own, file 0's.
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
[ "$(grep -c ' _helper$' $t/nm)" = 1 ]
[ "$(grep -c ' _other$' $t/nm)" = 1 ]
grep -q '\[  0\] _other$' $t/map
[ "$(grep -c '_helper$' $t/map)" = 1 ]

$CC --ld-path=$mold -o $t/exe2 $t/main.o $t/a1.o $t/a2.o $t/a3.o -Wl,-no_deduplicate
[ "$(nm $t/exe2 | grep -c ' _helper$')" = 2 ]

# With the private extern first, the locals fold into it.
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/a4.o $t/a1.o $t/a2.o $t/a3.o -Wl,-deduplicate
$t/exe3
nm -m $t/exe3 | grep ' _helper$' > $t/nm3
[ "$(grep -c . $t/nm3)" = 2 ]
grep -q 'non-external (was a private external) _helper$' $t/nm3

# The name of the private extern is listed for a local only if the
# first local to fold into it has it: here _other comes first.
$CC --ld-path=$mold -o $t/exe4 $t/main.o $t/a4.o $t/a3.o $t/a1.o $t/a2.o -Wl,-deduplicate
$t/exe4
nm -m $t/exe4 > $t/nm4
[ "$(grep -c ' _helper$' $t/nm4)" = 1 ]
grep -q 'non-external (was a private external) _helper$' $t/nm4
grep -q ' _other$' $t/nm4

# A folded function's other labels keep their file in -map; only the
# alias named as the function was is the linker's.
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
grep -q '\[  3\] _y5$' $t/map5
grep -q '\[  0\] _x5$' $t/map5
