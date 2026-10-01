#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime makes each record of a section of fixed-size records a
# subsection of its own, as each pointer of an Objective-C class list:
# its -map lists them one by one, "anon". Category merging rebuilds a
# category list without the categories it merged into their classes;
# ld-prime keeps the other entries, each its file's, and credits itself
# with the __objc_nlclslist entry it adds for a class a category's
# +load makes non-lazy. The merged categories' entries are dead.
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

# The map's rows for a section, without their addresses.
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

# Foo (Cat)'s entries in __objc_catlist and __objc_nlcatlist are dead.
[ "$(grep -c $'^<<dead>>\t0x00000008\t\\[  1\\] anon$' $t/map)" = 2 ]

# Merging gives a class lists of kinds it had none of: instance and
# class methods, a protocol list (the class's and the metaclass's) and
# an instance property list. ld-prime lists a dead pointer-sized
# subsection of the class's file for each list pointer so set, after
# the file's other dead subsections, and the merged property list,
# which no symbol names, as a subsection of its own.
cat <<EOF | $CC -o $t/b.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@protocol P - (int)a; @end
@interface Foo : NSObject @end
@implementation Foo @end
int dead_fn(void) { return 7; }
EOF
cat <<EOF | $CC -o $t/c.o -c -xobjective-c -
#import <Foundation/Foundation.h>
@protocol P - (int)a; @end
@interface Foo : NSObject @end
@interface Foo (Cat) <P> @property (readonly) int b; - (int)a; + (int)c; @end
@implementation Foo (Cat) - (int)a { return 1; } - (int)b { return 2; } + (int)c { return 3; } @end
int main() { return [[Foo new] a] + [[Foo new] b] + [Foo c]; }
EOF
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/c.o -framework Foundation -Wl,-dead_strip \
  -Wl,-map,$t/map2
sed -n '/^# Dead Stripped Symbols:/,$p' $t/map2 | grep '\[  1\]' | cut -f3 > $t/dead2
diff - $t/dead2 <<EOF
[  1] _dead_fn
[  1] anon
[  1] anon
[  1] anon
[  1] anon
[  1] anon
EOF
[ "$(grep -c $'^<<dead>>\t0x00000008\t\\[  1\\] anon$' $t/map2)" = 5 ]
sed -n '/^# Symbols:/,/^$/p' $t/map2 | grep -q $'\t0x00000018\t\\[  0\\] anon$'
