#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime makes each record of a section of fixed-size records an atom
# of its own, as each pointer of an Objective-C class list: its -map
# lists them one by one, "anon". Category merging rebuilds a category
# list without the categories it merged into their classes; ld-prime
# keeps the other entries, each its file's, and credits itself with
# the __objc_nlclslist entry it adds for a class a category's +load
# makes non-lazy.
cat <<EOF | $CC -o $t/a.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@interface Foo : NSObject - (int)a; + (void)cm; @end
@implementation Foo - (int)a { return 1; } + (void)cm {} @end
@interface Bar : NSObject - (int)a; @end
@implementation Bar - (int)a { return 2; } @end
@interface Foo (Cat) + (void)load; @end
@implementation Foo (Cat) + (void)load {} @end
@interface NSString (StrCat) - (int)zz; @end
@implementation NSString (StrCat) - (int)zz { return 3; } @end
int main() { return 0; }
EOF

$CC --ld-path=$mold -o $t/exe $t/a.o -framework Foundation -Wl,-dead_strip -Wl,-map,$t/map

# The map's rows for a section's atoms, without their addresses.
rows() {
  local sect start end addr size rest
  sect=$(grep $'\t'"$1"'$' $t/map)
  start=$(( $(echo "$sect" | cut -f1) ))
  end=$(( start + $(echo "$sect" | cut -f2) ))
  sed -n '/^# Symbols:/,/^$/p' $t/map | grep '^0x' |
    while IFS=$'\t' read -r addr size rest; do
      if (( addr >= start && addr < end )); then
        echo "$size $rest"
      fi
    done | tr '\n' '|'
}

[ "$(rows __objc_classlist)" = '0x00000008 [  1] anon|0x00000008 [  1] anon|' ]
[ "$(rows __objc_catlist)" = '0x00000008 [  1] anon|' ]
[ "$(rows __objc_nlclslist)" = '0x00000008 [  0] anon|' ]
grep $'\t__objc_nlcatlist$' $t/map > $t/nlcatlist || true
[ ! -s $t/nlcatlist ]
