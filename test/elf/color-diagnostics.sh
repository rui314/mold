#!/bin/bash
export LANG=
set -e
cd "$(dirname "$0")"
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh "$0") ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh "$0")
mkdir -p "$t"

cat <<EOF | cc -o "$t"/a.o -c -xc -
int foo();
int main() { foo(); }
EOF

! "$mold" -o "$t"/exe "$t"/a.o --color-diagnostics 2> "$t"/log
grep -Pq '\e' "$t"/log

! "$mold" -o "$t"/exe "$t"/a.o --color-diagnostics=always 2> "$t"/log
grep -Pq '\e' "$t"/log

! "$mold" -o "$t"/exe "$t"/a.o --color-diagnostics=never 2> "$t"/log
! grep -Pq '\e' "$t"/log || false

! "$mold" -o "$t"/exe "$t"/a.o --color-diagnostics=auto 2> "$t"/log
! grep -Pq '\e' "$t"/log || false

echo OK
