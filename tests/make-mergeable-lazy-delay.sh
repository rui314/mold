#!/bin/bash
source "$(dirname "$0")"/common.inc

# Code that reaches a symbol of a lazy-load or delay-init dylib is
# rewritten to go through a helper, which a mergeable dylib would keep
# under the compiler's fixup, so no link could merge it. mold refuses
# the combination; ld-prime aborts on -lazy-l (with no message) and
# makes an unmergeable dylib of -delay-l, so this test is mold's own.
cat <<EOF | $CC -o $t/bar.o -c -xc -
int bv = 10;
int bar(void) { return 4; }
EOF
cat <<EOF | $CC -o $t/foo.o -c -xc -
extern int bv;
int bar(void);
int foo(void) { return bar() + bv; }
EOF

mkdir -p $t/lib $t/Frameworks/Bar.framework
$CC --ld-path=$mold -shared -o $t/lib/libbar.dylib $t/bar.o \
  -Wl,-install_name,@rpath/libbar.dylib
$CC --ld-path=$mold -shared -o $t/Frameworks/Bar.framework/Bar $t/bar.o \
  -Wl,-install_name,@rpath/Bar.framework/Bar

for opt in -delay-lbar -delay_library,$t/lib/libbar.dylib -delay_framework,Bar; do
  not $CC --ld-path=$mold -shared -o $t/foo.dylib $t/foo.o -L$t/lib -F$t/Frameworks \
    -Wl,-make_mergeable -Wl,$opt 2> $t/log
  grep -q -- '-delay-l/-delay_library/-delay_framework cannot be used with -make_mergeable' $t/log
done

for opt in -lazy-lbar -lazy_library,$t/lib/libbar.dylib -lazy_framework,Bar; do
  not $CC --ld-path=$mold -shared -o $t/foo.dylib $t/foo.o -L$t/lib -F$t/Frameworks \
    -Wl,-make_mergeable -Wl,$opt -mmacosx-version-min=27.0 2> $t/log
  grep -q -- '-lazy-l/-lazy_library/-lazy_framework cannot be used with -make_mergeable' $t/log
done

# Before macOS 27, a lazy-load dylib links as any other.
$CC --ld-path=$mold -shared -o $t/foo.dylib $t/foo.o -L$t/lib -Wl,-make_mergeable \
  -Wl,-lazy-lbar -mmacosx-version-min=26.0 2> $t/log
grep -q "lazy-load will be ignored for 'bar'" $t/log
