#!/bin/bash
source "$(dirname "$0")"/common.inc

# A symbol found through a dylib's re-exports binds to the dylib that
# defines it when that dylib lives in a public location (a top-level
# /System/Library/Frameworks framework, /usr/lib/lib*.dylib), which
# then gets an implicit load command after the explicitly named
# libraries; a private re-exported library's symbols bind to the
# re-exporting dylib. AppKit re-exports Foundation (public; Foundation
# re-exports CoreFoundation, public) and UIFoundation (private):
# ld-prime binds NSHomeDirectory to Foundation, CFRelease to
# CoreFoundation and NSAttachmentAttributeName to AppKit, and lists
# AppKit, libSystem, CoreFoundation, Foundation (the command-line
# libraries in order, then the implicit ones by name).
# -no_implicit_dylibs binds everything to AppKit.
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
$t/exe
otool -L $t/exe | tail -n +2 | awk '{print $1}' | sed 's|.*/||' > $t/loads
[ "$(tr '\n' ' ' < $t/loads)" = "AppKit libSystem.B.dylib CoreFoundation Foundation " ]
dyld_info -fixups $t/exe | grep bind | awk '{print $NF}' | sort -u > $t/binds
grep -q '^AppKit/_NSApp$' $t/binds
grep -q '^AppKit/_NSAttachmentAttributeName$' $t/binds
grep -q '^Foundation/_NSHomeDirectory$' $t/binds
grep -q '^CoreFoundation/_CFRelease$' $t/binds

$CC --ld-path=$mold -o $t/exe2 $t/a.o -framework AppKit -Wl,-no_implicit_dylibs
$t/exe2
[ "$(otool -L $t/exe2 | tail -n +2 | awk '{print $1}' | sed 's|.*/||' | tr '\n' ' ')" = "AppKit libSystem.B.dylib " ]
dyld_info -fixups $t/exe2 | grep bind | awk '{print $NF}' | sort -u > $t/binds2
grep -q '^AppKit/_NSHomeDirectory$' $t/binds2
grep -q '^AppKit/_CFRelease$' $t/binds2

# Named explicitly as well: an explicit library keeps its command-line
# position even when a re-export reached it first.
$CC --ld-path=$mold -o $t/exe3 $t/a.o -framework AppKit -framework Foundation
$t/exe3
[ "$(otool -L $t/exe3 | tail -n +2 | awk '{print $1}' | sed 's|.*/||' | tr '\n' ' ')" = "AppKit Foundation libSystem.B.dylib CoreFoundation " ]
