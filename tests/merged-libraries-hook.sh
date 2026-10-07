#!/bin/bash
source "$(dirname "$0")"/common.inc

# Xcode keeps a mergeable framework's bundle, with its resources, in
# the app where an image merges the framework (-merge_framework) or
# re-exports it from elsewhere (-no_merge_framework), and the linker
# adds a hook for the framework's classes: in an app, the Objective-C
# runtime then names the framework's binary in the app as their image,
# and +[NSBundle bundleForClass:] (Swift's Bundle(for:)) finds the
# framework's bundle. A merged library's classes are those of its
# class list, hidden ones too; a re-exported one's are those it
# exports. A debug build of the framework (-add_mergeable_debug_hook)
# gets a hook for the classes it doesn't export.
cat <<EOF | $CC -o $t/foo.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface FooObjC : NSObject
@end
@implementation FooObjC
@end
__attribute__((visibility("hidden")))
@interface FooHidden : NSObject
@end
@implementation FooHidden
@end
EOF
objs=$t/foo.o
classes="FooObjC FooHidden"
swiftlib=$(xcrun --show-sdk-path)/usr/lib/swift
if command -v swiftc >/dev/null; then
  cat > $t/foo.swift <<EOF
import Foundation
public class FooSwift {}
class FooInternal {}
@_cdecl("foo_swift_bundle")
public func fooSwiftBundle() -> UnsafeMutablePointer<CChar> {
  strdup(Bundle(for: FooSwift.self).bundlePath)
}
EOF
  swiftc -target $ARCH-apple-macos14.0 -parse-as-library -module-name Foo -c \
    -o $t/fooswift.o $t/foo.swift
  objs="$objs $t/fooswift.o"
  classes="$classes _TtC3Foo8FooSwift _TtC3Foo11FooInternal"
fi

# Prints the bundle each class names, by its path in the app, and
# whether the bundle has the resource, then what Bundle(for:) says; and
# first, from an initializer, the bundle FooObjC names then.
cat <<EOF | $CC -o $t/main.o -c -xobjective-c -
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#include <dlfcn.h>
static const char *where(NSBundle *b) {
  if (b == NSBundle.mainBundle)
    return "main";
  // The main bundle's path is the real one, a framework's the one dyld
  // loaded it by (e.g. /tmp rather than /private/tmp).
  NSString *app = NSBundle.mainBundle.bundlePath.stringByResolvingSymlinksInPath;
  NSString *path = b.bundlePath.stringByResolvingSymlinksInPath;
  return [path stringByReplacingOccurrencesOfString:app withString:@"@app"].UTF8String;
}
__attribute__((constructor)) static void init(void) {
  printf("init %s\n", where([NSBundle bundleForClass:objc_getClass("FooObjC")]));
}
int main(int argc, char **argv) {
  @autoreleasepool {
    for (int i = 1; i < argc; i++) {
      NSBundle *b = [NSBundle bundleForClass:objc_getClass(argv[i])];
      printf("%s %s %s\n", argv[i], where(b), [b pathForResource:@"res" ofType:@"txt"] ? "res" : "-");
    }
    char *(*swift_bundle)(void) = dlsym(RTLD_DEFAULT, "foo_swift_bundle");
    if (swift_bundle)
      printf("Bundle(for:) %s\n", where([NSBundle bundleWithPath:@(swift_bundle())]));
  }
}
EOF

# Lays a framework out in directory $1 with binary $2.
make_framework() {
  rm -rf $1/Foo.framework
  mkdir -p $1/Foo.framework/Versions/A
  cp $2 $1/Foo.framework/Versions/A/Foo
  ln -s A $1/Foo.framework/Versions/Current
  ln -s Versions/Current/Foo $1/Foo.framework/Foo
}

$CC -shared -o $t/Foo $objs -framework Foundation -L$swiftlib -Wl,-make_mergeable \
  -Wl,-install_name,@rpath/Foo.framework/Versions/A/Foo
make_framework $t/lib $t/Foo

# Makes MyApp.app with Foo's bundle (and with binary $1, Foo's binary in
# ReexportedBinaries, as Xcode's debug builds have it), links it with
# the other arguments, and runs it.
run_app() {
  local app=$t/MyApp.app/Contents binary=$1
  shift
  rm -rf $t/MyApp.app
  mkdir -p $app/MacOS $app/Frameworks/ReexportedBinaries
  make_framework $app/Frameworks $t/Foo
  mkdir $app/Frameworks/Foo.framework/Versions/A/Resources
  echo resource > $app/Frameworks/Foo.framework/Versions/A/Resources/res.txt
  ln -s Versions/Current/Resources $app/Frameworks/Foo.framework/Resources
  [ -z "$binary" ] || make_framework $app/Frameworks/ReexportedBinaries $binary
  cat > $app/Info.plist <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>MyApp</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
EOF
  $CC --ld-path=$mold -o $app/MacOS/MyApp $t/main.o -framework Foundation -L$swiftlib \
    -Wl,-rpath,@executable_path/../Frameworks/ReexportedBinaries "$@"
  $app/MacOS/MyApp $classes > $t/out
}

# Each class is where $1 says, and the hidden ones where $2 does if
# given; and so is FooSwift for Bundle(for:).
check() {
  for class in $classes; do
    case $class in
    FooHidden|*FooInternal) grep -q "^$class ${2:-$1}$" $t/out ;;
    *) grep -q "^$class $1$" $t/out ;;
    esac
  done
  if command -v swiftc >/dev/null; then
    grep -q "^Bundle(for:) ${1% *}$" $t/out
  fi
}

# Merged, the classes are in the app's binary, but the hook places them
# in Foo's bundle; outside an app, it does nothing.
in_app='@app/Contents/Frameworks/Foo.framework res'
reexported='@app/Contents/Frameworks/ReexportedBinaries/Foo.framework -'
run_app "" -F$t/lib -Wl,-merge_framework,Foo
check "$in_app"
rm -rf $t/plain && mkdir $t/plain
cp $t/MyApp.app/Contents/MacOS/MyApp $t/plain/MyApp
$RUN $t/plain/MyApp $classes > $t/out
check 'main -'
run_app "" -F$t/lib -Wl,-merge_framework,Foo -Wl,-no_merged_libraries_hook
check 'main -'

# The hook's object comes first, so it is installed before the image's
# initializers run from __mod_init_func, but after them from
# __init_offsets (as chained fixups have it).
run_app "" -F$t/lib -Wl,-merge_framework,Foo -Wl,-fixup_chains
grep -q '^init main$' $t/out
run_app "" -F$t/lib -Wl,-merge_framework,Foo -Wl,-no_fixup_chains
check "$in_app"
grep -q '^init @app/Contents/Frameworks/Foo.framework$' $t/out

# Re-exported, the classes are in Foo's binary elsewhere; the hook is
# for the exported ones.
run_app $t/Foo -F$t/lib -Wl,-no_merge_framework,Foo
check "$in_app" "$reexported"
run_app $t/Foo -F$t/lib -Wl,-no_merge_framework,Foo -Wl,-no_merged_libraries_hook
check "$reexported"

# A debug build of Foo places its hidden classes itself, under the last
# component of its install name. -no_merged_libraries_hook drops that
# hook too.
$CC --ld-path=$mold -shared -o $t/FooDebug $objs -framework Foundation -L$swiftlib \
  -Wl,-install_name,@rpath/Foo.framework/Versions/A/Foo -Wl,-add_mergeable_debug_hook
make_framework $t/debug $t/FooDebug
run_app $t/FooDebug -F$t/debug -Wl,-no_merge_framework,Foo
check "$in_app"
$CC --ld-path=$mold -shared -o $t/FooDebug $objs -framework Foundation -L$swiftlib \
  -Wl,-install_name,@rpath/Foo.framework/Versions/A/Foo -Wl,-add_mergeable_debug_hook \
  -Wl,-no_merged_libraries_hook
run_app $t/FooDebug -F$t/debug -Wl,-no_merge_framework,Foo
check "$in_app" "$reexported"
