#!/bin/bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fcommon -xc -c -o $t/a.o -
int foo;
EOF

cat <<EOF | $CC -fcommon -xc -c -o $t/b.o -
int foo;
int main() { return 0; }
EOF

# Control: -warn-common emits a warning by default
$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-warn-common 2> $t/log
grep -q "multiple common symbols" $t/log

# -w suppresses the warning
$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-warn-common -Wl,-w 2> $t/log
not grep -q "multiple common symbols" $t/log

# --no-warnings does the same
$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-warn-common -Wl,--no-warnings 2> $t/log
not grep -q "multiple common symbols" $t/log

# Sanity: -fatal-warnings without -w would have failed
not $CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-warn-common -Wl,-fatal-warnings 2> /dev/null

# -w cancels -fatal-warnings (lld parity)
$CC -B. -o $t/exe $t/a.o $t/b.o -Wl,-warn-common -Wl,-fatal-warnings -Wl,-w
