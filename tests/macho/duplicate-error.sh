#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void hello() {}
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
void hello() {}
int main() {}
EOF

# mold reports each definition that lost to the first one, naming its
# file, the winner's and the symbol. (ld-prime lists every defining
# file under "duplicate symbol '_hello' in:", then counts the symbols.)
! $CC --ld-path=$mold -o $t/exe $t/a.o $t/b.o 2> $t/log || false
grep -q 'duplicate symbol' $t/log
grep -q _hello $t/log
if is_mold; then
  grep -q "^mold: error: duplicate symbol: $t/b.o: $t/a.o: _hello\$" $t/log
fi

# A relocatable link must fail before publishing an erroneous object.
# Exercise both the default parent/child protocol and the debugger mode.
for no_fork in no yes; do
  rm -f $t/merged.o
  if [ "$no_fork" = yes ]; then
    ! MOLD_NO_FORK=1 $mold -r -arch $ARCH -o $t/merged.o $t/a.o $t/b.o 2> $t/log || false
  else
    ! $mold -r -arch $ARCH -o $t/merged.o $t/a.o $t/b.o 2> $t/log || false
  fi
  grep -q 'duplicate symbol.*_hello' $t/log
  test ! -e $t/merged.o
done

# Required output work must finish before the parent reports success:
# a -dependency_info file that can't be created is only a warning, but
# -fatal_warnings makes it fail the link.
! $mold -r -arch $ARCH -o $t/merged.o $t/a.o -dependency_info $t/missing/dep -fatal_warnings \
  2> $t/log || false
grep -q 'Could not open or create -dependency_info file: .*missing/dep' $t/log
