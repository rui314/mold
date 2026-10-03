#!/bin/bash
source "$(dirname "$0")"/common.inc

# A merged mergeable dylib built for a newer macOS than the link gets
# the warning of a dylib, not an object's, whatever
# -deployment_target_mismatches says. (ld-prime warns again as it
# merges its code.)
cat <<EOF | $CC -o $t/a.o -c -xc - -mmacosx-version-min=26.0
int foo(void) { return 3; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc - -mmacosx-version-min=15.0
int bar(void) { return 4; }
EOF
cat <<EOF | $CC -o $t/main.o -c -xc - -mmacosx-version-min=14.0
int foo(void);
int bar(void);
int main() { return foo() + bar(); }
EOF

mkdir -p $t/Frameworks/Foo.framework
$CC --ld-path=$mold -shared -o $t/Frameworks/Foo.framework/Foo $t/a.o \
  -Wl,-install_name,@rpath/Foo.framework/Foo -Wl,-make_mergeable -mmacosx-version-min=26.0

$CC --ld-path=$mold -o $t/exe $t/main.o -F$t/Frameworks -Wl,-merge_framework,Foo \
  $t/b.o -mmacosx-version-min=14.0 -Wl,-deployment_target_mismatches,warning 2> $t/log
sed 's/^[a-z]*: warning: //; s|([^()]*/b.o)|(b.o)|' $t/log > $t/log2
dylib="building for macOS-14.0, but linking with dylib '@rpath/Foo.framework/Foo' which was built for newer version 26.0"
obj="object file (b.o) was built for newer 'macOS' version (15.0) than being linked (14.0)"
printf '%s\n' "$dylib" "$obj" | sort | diff - <(sort $t/log2)

$CC --ld-path=$mold -o $t/exe $t/main.o -F$t/Frameworks -Wl,-merge_framework,Foo \
  $t/b.o -mmacosx-version-min=14.0 -Wl,-deployment_target_mismatches,suppress 2> $t/log3
sed 's/^[a-z]*: warning: //' $t/log3 > $t/log4
printf '%s\n' "$dylib" | diff - $t/log4
