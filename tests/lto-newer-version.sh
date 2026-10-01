#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -flto -mmacosx-version-min=27.0 -c -xc - -o $t/a.o
int helper(void);
int helper2(void);
int main(void) { return helper() + helper2() - 6; }
EOF
echo 'int helper(void) { return 3; }' | $CC -mmacosx-version-min=27.0 -c -xc - -o $t/b.o
echo 'int helper2(void) { return 3; }' | $CC -flto -mmacosx-version-min=26.5 -c -xc - -o $t/c.o

# ld-prime checks a bitcode file's target triple as it checks a Mach-O
# object's platform load command, in input order, and the object LTO
# compiled last of all.
$CC --ld-path=$mold -flto -mmacosx-version-min=26.0 -o $t/exe $t/a.o $t/b.o $t/c.o 2> $t/log
sed -n 's/.*object file (\(.*\)) was built for newer .* version (\(.*\)) than .*/\1 \2/p' \
  $t/log > $t/files
cat <<EOF > $t/expected
$(pwd -P)/$t/a.o 27.0
$(pwd -P)/$t/b.o 27.0
$(pwd -P)/$t/c.o 26.5
/tmp/lto.o 27.0
EOF
diff $t/files $t/expected
$t/exe

# A -r link of bitcode alone checks the bitcode too.
$CC --ld-path=$mold -flto -mmacosx-version-min=26.0 -r -o $t/r.o $t/a.o $t/c.o 2> $t/log2
grep -q "object file ($(pwd -P)/$t/a.o) was built for newer 'macOS' version (27.0)" $t/log2
