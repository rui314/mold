#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
OBJDUMP="${OBJDUMP:-objdump}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
cd "$(dirname "$0")"/../..
t=out/test/elf/$testname
mkdir -p $t

# DSO with an undefined symbol that is referenced
cat <<'EOF' | $CC -shared -fPIC -o $t/liba.so -xc -
void fn2();
void fn1() { fn2(); }
void fn3() {}
EOF

# call a function from `liba.so` which uses an undefined symbol
cat <<'EOF' > $t/b.c
extern void fn1();
int main() {
  fn1();
  return 0;
}
EOF

# default is `--allow-shlib-undefined` for `-shared`
$CC -B. -shared -fPIC -o $t/libb.so $t/b.c -Wl,-allow-shlib-undefined $t/liba.so

# default is `--no-allow-shlib-undefined` for executables
output=$($CC -B. -o $t/b $t/b.c -Wl,--no-allow-shlib-undefined $t/liba.so 2>&1 || :)
echo $output | grep -Eq 'undefined symbol: fn2'

# fails because `fn2` is undefined in `liba.so`
output=$($CC -B. -shared -fPIC -o $t/libb.so $t/b.c -Wl,--no-allow-shlib-undefined $t/liba.so 2>&1 || :)
echo $output | grep -Eq 'undefined symbol: fn2'

$CC -B. -o $t/b $t/b.c -Wl,--allow-shlib-undefined $t/liba.so

cat <<'EOF' | $CC -shared -fPIC -o $t/libfn2.so -xc -
void fn2() {}
EOF

# DSO with an undefined symbol that is defined in one of its dependencies
cat <<'EOF' | $CC -shared -Wl,-rpath,"\$ORIGIN/" -fPIC -o $t/libfn.so -L$(realpath $t) -Wl,--no-as-needed -lfn2 -xc -
void fn2();
void fn1() { fn2(); }
void fn3() {}
EOF

# works because `fn2` is defined in `libfn2.so` which is NEEDED by `libfn.so`
$CC -B. -o $t/b $t/b.c -L$(realpath $t) $t/libfn.so

# DSO with an undefined symbol that is not referenced
cat <<'EOF' | $CC -shared -fPIC -o $t/liba2.so -xc -
void fn2();
void fn1() {}
void fn3() {}
EOF

# works because even though `fn2` is undefined, `b.o` doesn't reference it
$CC -B. -o $t/b $t/b.c $t/liba2.so

echo OK
