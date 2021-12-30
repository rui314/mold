#!/bin/bash
export LANG=
set -e
testname=$(basename -s .sh "$0")
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
mold="$(pwd)/mold"
t="$(pwd)/out/test/elf/$testname"
mkdir -p "$t"

if [ "$(uname -m)" = x86_64 ]; then
  dialect=gnu
elif [ "$(uname -m)" = aarch64 ]; then
  dialect=trad
else
  echo skipped
  exit 0
fi

echo '{ global: bar; local: *; };' > "$t"/a.ver

cat <<EOF | gcc -mtls-dialect=$dialect -fPIC -c -o "$t"/b.o -xc -
_Thread_local int foo;

int bar() {
  return foo;
}
EOF

clang -fuse-ld="$mold" -shared -o "$t"/c.so "$t"/b.o -Wl,--version-script="$t"/a.ver \
  -Wl,--no-relax

readelf -W --dyn-syms "$t"/c.so | grep -Pq 'TLS     LOCAL  DEFAULT   \d+ foo'

echo OK
