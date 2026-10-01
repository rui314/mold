#!/bin/bash
source "$(dirname "$0")"/common.inc

# An N_OSO stab names an object with debug info, and its value is the
# object's modification time, which dsymutil and lldb check the file
# they find against. An archive member's is the time its ar header
# records. ZERO_AR_DATE, set to any value, makes ld-prime write 0 for
# every one of them, for reproducible builds.
echo 'int am(void) { return 3; }' | $CC -o $t/am.o -c -g -xc -
touch -t 202001020304.05 $t/am.o
rm -f $t/libam.a
ar rcs $t/libam.a $t/am.o
echo 'int am(void); int main(void) { return am(); }' | $CC -o $t/main.o -c -g -xc -

osos() { nm -ap $1 | awk '$5 == "OSO" { printf "%s ", $1 }'; }
member=$(printf '%016x' $(stat -f %m $t/am.o))
object=$(printf '%016x' $(stat -f %m $t/main.o))

$CC --ld-path=$mold -o $t/exe $t/main.o $t/libam.a
[ "$(osos $t/exe)" = "$object $member " ]

ZERO_AR_DATE=1 $CC --ld-path=$mold -o $t/exe2 $t/main.o $t/libam.a
[ "$(osos $t/exe2)" = '0000000000000000 0000000000000000 ' ]

ZERO_AR_DATE= $mold -arch $ARCH -r $t/main.o -o $t/r.o
[ "$(osos $t/r.o)" = '0000000000000000 ' ]

# So does -reproducible.
$CC --ld-path=$mold -o $t/exe3 $t/main.o $t/libam.a -Wl,-reproducible
[ "$(osos $t/exe3)" = '0000000000000000 0000000000000000 ' ]
$mold -arch $ARCH -r $t/main.o -o $t/r2.o -reproducible
[ "$(osos $t/r2.o)" = '0000000000000000 ' ]
