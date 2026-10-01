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
# (ld-prime lists the files in no stable order.)
grep -v '^+' $t/log | sed -E 's|^(ld: \|mold: error: )||; s|/.*/||' > $t/msgs
cat > $t/expected <<EOF
duplicate symbol '_hello' in:
    a.o
    b.o
1 duplicate symbols
EOF
grep -v 'linker command failed' $t/msgs | sort | diff - <(sort $t/expected)

# A relocatable link must fail before publishing an erroneous object.
# Exercise both the default parent/child protocol and the debugger mode.
for no_fork in no yes; do
  rm -f $t/merged.o
  if [ "$no_fork" = yes ]; then
    ! MOLD_NO_FORK=1 $mold -r -arch $ARCH -o $t/merged.o $t/a.o $t/b.o 2> $t/log || false
  else
    ! $mold -r -arch $ARCH -o $t/merged.o $t/a.o $t/b.o 2> $t/log || false
  fi
  grep -q "duplicate symbol '_hello' in:" $t/log
  test ! -e $t/merged.o
done

# Required output work must finish before the parent reports success:
# a -dependency_info file that can't be created is only a warning, but
# -fatal_warnings makes it fail the link.
! $mold -r -arch $ARCH -o $t/merged.o $t/a.o -dependency_info $t/missing/dep -fatal_warnings \
  2> $t/log || false
grep -q 'Could not open or create -dependency_info file: .*missing/dep' $t/log
