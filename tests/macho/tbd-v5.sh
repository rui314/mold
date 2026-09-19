#!/bin/bash
source "$(dirname "$0")"/common.inc

# TBD version 5 is JSON. It is what tapi writes today and what Xcode
# generates as the "eager linking" stub of a framework built in the
# same workspace (Stats' Kit.framework, Sparkle, Hammerspoon's
# LuaSkin), so a dependent target can link before the framework is
# built. Symbols come per target group, split into data and text, with
# Objective-C classes and thread-locals listed by kind; reexported
# libraries may be inlined in a "libraries" array.
mkdir -p $t/libs/Some.framework/
cat > $t/libs/Some.framework/Some.tbd <<'EOF'
{
  "main_library": {
    "install_names": [{"name": "@rpath/Some.framework/Versions/A/Some"}],
    "current_versions": [{"version": "2.1"}],
    "target_info": [{"target": "arm64-macos", "min_deployment": "13"},
                    {"target": "x86_64-macos", "min_deployment": "13"}],
    "flags": [{"attributes": ["not_app_extension_safe"]}],
    "exported_symbols": [
      {"data": {"global": ["_some_data"], "weak": ["_some_weak"],
                "thread_local": ["_some_tls"], "objc_class": ["SomeClass"]},
       "text": {"global": ["_some_func"]}}
    ],
    "reexported_libraries": [{"names": ["@rpath/Inner.framework/Versions/A/Inner",
                                        "/usr/lib/libElsewhere.dylib"]}]
  },
  "libraries": [
    {"install_names": [{"name": "@rpath/Inner.framework/Versions/A/Inner"}],
     "target_info": [{"target": "arm64-macos", "min_deployment": "13"},
                     {"target": "x86_64-macos", "min_deployment": "13"}],
     "exported_symbols": [{"text": {"global": ["_inner_func"]}}]}
  ],
  "tapi_tbd_version": 5
}
EOF

cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
extern int some_data;
extern int some_weak __attribute__((weak_import));
extern _Thread_local int some_tls;
void some_func(void);
void inner_func(void);
@interface SomeClass : NSObject @end
int main() {
  some_func();
  inner_func();
  [SomeClass class];
  return some_data + some_tls + (&some_weak ? 1 : 0);
}
EOF

# The stub resolves every reference; the external re-export is not
# present, which is only a warning.
$CC --ld-path=$mold -o $t/exe $t/a.o -F$t/libs -framework Some -framework Foundation \
  -Wl,-w
otool -L $t/exe | grep -q 'Some.framework/Versions/A/Some (compatibility version 1.0.0, current version 2.1.0)'
nm -m $t/exe > $t/nm
grep -q 'undefined.*_some_func (from Some)' $t/nm
grep -q 'undefined.*_inner_func (from Some)' $t/nm
grep -q 'undefined.*_OBJC_CLASS_\$_SomeClass (from Some)' $t/nm
grep -q 'undefined.*weak.*_some_weak (from Some)' $t/nm

# An app extension may not link a library flagged not_app_extension_safe.
$CC --ld-path=$mold -o $t/exe2 $t/a.o -F$t/libs -framework Some -framework Foundation \
  -Wl,-application_extension 2> $t/log || true
grep -q 'not safe for use in application extensions' $t/log
