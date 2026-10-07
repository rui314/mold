#!/bin/bash
source "$(dirname "$0")"/common.inc

# A symbol found through a dylib's re-exports binds to the dylib that
# defines it when that dylib lives in a public location (a top-level
# /System/Library/Frameworks framework, /usr/lib/lib*.dylib), which
# then gets an implicit load command of its own; a private re-exported
# library's symbols bind to the re-exporting dylib. AppKit re-exports
# Foundation (public; Foundation re-exports CoreFoundation, public) and
# UIFoundation (private): NSHomeDirectory binds to Foundation, CFRelease
# to CoreFoundation and NSAttachmentAttributeName to AppKit, and the
# command-line libraries keep their order. (ld-prime lists the implicit
# ones last, by name.) -no_implicit_dylibs binds everything to AppKit.
cat <<EOF2 | $CC -o $t/a.o -c -xc -
typedef const void *CFTypeRef;
void CFRelease(CFTypeRef);
CFTypeRef CFRetain(CFTypeRef);
void *NSHomeDirectory(void);
extern void *NSApp;
extern void *NSAttachmentAttributeName;
int main() {
  void *h = NSHomeDirectory();
  CFRelease(CFRetain(h));
  return !(NSApp == 0 && NSAttachmentAttributeName != 0);
}
EOF2
$CC --ld-path=$mold -o $t/exe $t/a.o -framework AppKit -mmacosx-version-min=15.0
$RUN $t/exe
otool -L $t/exe | tail -n +2 | awk '{print $1}' | sed 's|.*/||' > $t/loads
[ "$(sort $t/loads | tr '\n' ' ')" = "AppKit CoreFoundation Foundation libSystem.B.dylib " ]
[ "$(grep -e '^AppKit$' -e '^libSystem' $t/loads | tr '\n' ' ')" = "AppKit libSystem.B.dylib " ]
dyld_info -fixups $t/exe | grep bind | awk '{print $NF}' | sort -u > $t/binds
grep -q '^AppKit/_NSApp$' $t/binds
grep -q '^AppKit/_NSAttachmentAttributeName$' $t/binds
grep -q '^Foundation/_NSHomeDirectory$' $t/binds
grep -q '^CoreFoundation/_CFRelease$' $t/binds

$CC --ld-path=$mold -o $t/exe2 $t/a.o -framework AppKit -Wl,-no_implicit_dylibs
$RUN $t/exe2
[ "$(otool -L $t/exe2 | tail -n +2 | awk '{print $1}' | sed 's|.*/||' | tr '\n' ' ')" = "AppKit libSystem.B.dylib " ]
dyld_info -fixups $t/exe2 | grep bind | awk '{print $NF}' | sort -u > $t/binds2
grep -q '^AppKit/_NSHomeDirectory$' $t/binds2
grep -q '^AppKit/_CFRelease$' $t/binds2

# Named explicitly as well: an explicit library keeps its command-line
# position even when a re-export reached it first.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -framework AppKit -framework Foundation
$RUN $t/exe3
otool -L $t/exe3 | tail -n +2 | awk '{print $1}' | sed 's|.*/||' > $t/loads3
[ "$(sort $t/loads3 | tr '\n' ' ')" = "AppKit CoreFoundation Foundation libSystem.B.dylib " ]
[ "$(grep -v CoreFoundation $t/loads3 | tr '\n' ' ')" = "AppKit Foundation libSystem.B.dylib " ]

# A framework's binary is told by its name: the path must end in the
# name before the first dot after /System/Library/Frameworks/, so a
# library inside a framework is no public one (OpenGL re-exports
# Libraries/libGL.dylib, whose symbols bind to OpenGL), nor a binary
# named otherwise.
cat <<EOF2 | $CC -o $t/gl.o -c -xc -
void glClear(unsigned);
int main() { glClear(0); return 0; }
EOF2
$CC --ld-path=$mold -o $t/exe4 $t/gl.o -framework OpenGL
otool -L $t/exe4 > $t/loads4
not grep -q libGL $t/loads4
dyld_info -fixups $t/exe4 | grep -q 'OpenGL/_glClear$'

mkdir -p $t/lib
for name in Bar.framework/Versions/A/XBar Foo.framework/Libraries/Bar; do
  cat > $t/lib/libfoo.tbd <<EOF2
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-$PLATFORM ]
install-name:    '/usr/lib/libfoo.dylib'
reexported-libraries:
  - targets:         [ $ARCH-$PLATFORM ]
    libraries:       [ '/System/Library/Frameworks/$name' ]
exports:
  - targets:         [ $ARCH-$PLATFORM ]
    symbols:         [ _foo ]
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-$PLATFORM ]
install-name:    '/System/Library/Frameworks/$name'
exports:
  - targets:         [ $ARCH-$PLATFORM ]
    symbols:         [ _bar ]
...
EOF2
  echo 'void bar(void); int main() { bar(); return 0; }' | $CC -o $t/b.o -c -xc -
  $CC --ld-path=$mold -o $t/exe5 $t/b.o -L$t/lib -lfoo
  dyld_info -fixups $t/exe5 | grep -q 'libfoo/_bar$'
done
