#!/bin/bash
source "$(dirname "$0")"/common.inc

# A library the link already holds as a public re-export (Foundation's
# stub brings CoreFoundation) that an object's auto-link option then
# names is a hint like any auto-linked library: ld-prime lists it only
# if something binds to it. Named on the command line instead, it is
# kept.
cat <<EOF | $CC -o $t/opt.o -c -xassembler -
.linker_option "-framework", "CoreFoundation"
EOF
cat <<EOF | $CC -o $t/a.o -c -xc -
void *NSHomeDirectory(void);
int main() { return NSHomeDirectory() == 0; }
EOF
cat <<EOF | $CC -o $t/b.o -c -xc -
typedef const void *CFTypeRef;
void CFRelease(CFTypeRef);
CFTypeRef CFRetain(CFTypeRef);
void *NSHomeDirectory(void);
int main() { CFRelease(CFRetain(NSHomeDirectory())); }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o $t/opt.o -framework Foundation
$t/exe
[ "$(otool -L $t/exe | tail -n +2 | awk '{print $1}' | sed 's|.*/||' | tr '\n' ' ')" = "Foundation libSystem.B.dylib " ]

$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/opt.o -framework Foundation
otool -L $t/exe2 | grep -q CoreFoundation.framework
dyld_info -fixups $t/exe2 | grep -q 'CoreFoundation/_CFRelease$'

$CC --ld-path=$mold -o $t/exe3 $t/a.o -framework Foundation -framework CoreFoundation
otool -L $t/exe3 | grep -q CoreFoundation.framework
