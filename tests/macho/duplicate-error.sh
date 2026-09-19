#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void hello() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void hello() {}
int main() {}
EOF

! $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o 2> $t/log || false
grep -q 'duplicate symbol: .*/b.o: .*/a.o: _hello' $t/log

# A relocatable link must fail before publishing an erroneous object.
# Exercise both the default parent/child protocol and the debugger mode.
for no_fork in no yes; do
  rm -f $t/merged.o
  if [ "$no_fork" = yes ]; then
    ! MOLD_NO_FORK=1 $mold -r -arch $ARCH -o $t/merged.o $t/a.o $t/b.o 2> $t/log || false
  else
    ! $mold -r -arch $ARCH -o $t/merged.o $t/a.o $t/b.o 2> $t/log || false
  fi
  grep -q 'duplicate symbol: .*_hello' $t/log
  test ! -e $t/merged.o
done

# Required output work must finish before the parent reports success.
! $mold -r -arch $ARCH -o $t/merged.o $t/a.o -dependency_info $t/missing/dep 2> $t/log || false
grep -q 'cannot open .*missing/dep' $t/log
