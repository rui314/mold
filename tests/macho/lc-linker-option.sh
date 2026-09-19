#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc - -fmodules
#include <zlib.h>
int main() {}
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o

# An auto-link option naming a library or framework that cannot be
# found is ignored without a word, as in ld64: header-only SDK
# frameworks such as CoreAudioTypes have a framework directory but no
# binary, and every Swift object importing one carries `-framework
# CoreAudioTypes` (CotEditor's build printed a warning per object).
mkdir -p $t/mm
cat <<EOF > $t/mm/module.modulemap
module Foo { header "foo.h" link framework "NoSuchFramework" link "nosuchlib" }
EOF
echo 'int foo(void);' > $t/mm/foo.h
cat <<EOF | $CC -o $t/b.o -c -xc - -fmodules -fmodules-cache-path=$t/mm/cache -I$t/mm
#include "foo.h"
int main() {}
EOF
otool -l $t/b.o | grep -A3 LC_LINKER_OPTION | grep -q 'string #1 -lnosuchlib'
$CC --ld-path=$mold -o $t/exe2 $t/b.o 2> $t/stderr
[ ! -s $t/stderr ]

# Auto-link options are acted on as a sorted set (ld64 lists the
# auto-linked libraries alphabetically, which fixes their ordinals),
# and an auto-linked library nothing binds to gets no load command:
# NetNewsWire's auto-link options name 43 frameworks and Swift overlays
# it never binds to, and ld-prime lists none of them.
mkdir -p $t/mm2
cat <<EOF > $t/mm2/module.modulemap
module Bar { header "bar.h" link "z" link framework "Foundation" link "resolv" link framework "CoreFoundation" }
EOF
cat <<EOF > $t/mm2/bar.h
typedef const void *CFTypeRef;
void CFRelease(CFTypeRef);
CFTypeRef CFRetain(CFTypeRef);
unsigned long crc32(unsigned long, const unsigned char *, unsigned);
void *NSHomeDirectory(void);
EOF
cat <<EOF | $CC -o $t/c.o -c -xc - -fmodules -fmodules-cache-path=$t/mm2/cache -I$t/mm2
#include "bar.h"
#include <stdio.h>
int main() {
  void *h = NSHomeDirectory();
  printf("%d %lu\n", h != 0, crc32(0, (const unsigned char *)"x", 1));
  CFRelease(CFRetain(h));
}
EOF
$CC --ld-path=$mold -o $t/exe3 $t/c.o
$t/exe3 | grep -q '^1 '
otool -L $t/exe3 | tail -n +2 | awk '{print $1}' > $t/libs
not grep -q resolv $t/libs
[ "$(sed 's|.*/||' $t/libs | tr '\n' ' ')" = "libSystem.B.dylib CoreFoundation Foundation libz.1.dylib " ]
