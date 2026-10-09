#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime turns most of its defaults on at a season's OS releases, the
# version sets ld64 names (version2019Fall: macOS 10.15 and iOS 13).
# tvOS is numbered as iOS is, a simulator as its device, and visionOS
# has every default up to its first release. Each check links just
# before and at a set's iOS or visionOS version.
plats='ios-simulator tvos-simulator xros-simulator'
[ $ARCH = arm64 ] && plats="ios tvos xros $plats"

sdk_name() {
  case $1 in
  ios) echo iphoneos ;;
  ios-simulator) echo iphonesimulator ;;
  tvos) echo appletvos ;;
  tvos-simulator) echo appletvsimulator ;;
  xros) echo xros ;;
  xros-simulator) echo xrsimulator ;;
  esac
}

# The versions just before and at a set, "-" for none before it.
versions() {
  case $os:$1 in
  xros:2024spring) echo 1.0 1.1 ;;
  xros:2024fall) echo 1.1 2.0 ;;
  xros:2026fall) echo 26.0 27.0 ;;
  xros:*) echo - 1.0 ;;
  *:2012fall) echo 5.1 6.0 ;;
  *:2013fall) echo 6.1 7.0 ;;
  *:2019fall) echo 12.4 13.0 ;;
  *:2020fall) echo 13.4 14.0 ;;
  *:2021fall) echo 14.0 15.0 ;;
  *:2024spring) echo 17.3 17.4 ;;
  *:2024fall) echo 17.4 18.0 ;;
  *:2026fall) echo 26.0 27.0 ;;
  esac
}

sects() { otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" { print $2 "," s }'; }
fmt() { dyld_info -fixup_chains $1 | grep -m1 -o 'pointer_format: *[0-9]*' | awk '{print $2}'; }

for name in $plats; do
  sdk=$(xcrun --sdk $(sdk_name $name) --show-sdk-path 2> /dev/null) || continue
  os=${name%-simulator}
  env=${name#$os}
  d=$t/$name
  mkdir -p $d
  cc="cc -target $ARCH-apple-${os}1.0$env -isysroot $sdk"
  [ $os = xros ] || cc="cc -target $ARCH-apple-${os}6.0$env -isysroot $sdk"

  cat <<EOF | $cc -o $d/a.o -c -xc - -femit-dwarf-unwind=always
int g = 1;
int *const gp = &g;
void start(void) __asm__("start");
void start(void) {}
int main() { return *gp - 1; }
EOF
  cat <<EOF | $cc -o $d/objc.o -c -xobjective-c -fno-objc-arc -
@interface Foo { void *isa; }
+ (id)alloc;
- (int)m;
@end
@implementation Foo
+ (id)alloc { return 0; }
- (int)m { return 1; }
@end
@interface Bar : Foo
@end
@implementation Bar
- (int)m { return [super m]; }
@end
int f() { return [[Bar alloc] m]; }
EOF
  echo 'int l(void) { return 0; }' | $cc -o $d/l.o -c -xc -
  $mold -arch $ARCH -platform_version $name 17.0 27.0 -syslibroot $sdk -lSystem -dylib \
    -install_name @rpath/libl.dylib $d/l.o -o $d/libl.dylib 2> /dev/null

  link() {
    local ver=$1 out=$2
    shift 2
    $mold -arch $ARCH -platform_version $name $ver 27.0 -syslibroot $sdk -lSystem $d/a.o \
      -o $d/$out "$@"
  }

  # LC_SOURCE_VERSION and LC_MAIN come with the 2012 releases; an older
  # x86-64 simulator image starts at "start" from LC_UNIXTHREAD.
  read old new <<< "$(versions 2012fall)"
  if [ $old != - ]; then
    link $old exe1 2> /dev/null
    otool -l $d/exe1 > $d/lc1
    not grep -q LC_SOURCE_VERSION $d/lc1
    if [ $ARCH = x86_64 ]; then
      grep -q LC_UNIXTHREAD $d/lc1
    else
      grep -q LC_MAIN $d/lc1
    fi
  fi
  link $new exe2 2> /dev/null
  otool -l $d/exe2 > $d/lc2
  grep -q LC_SOURCE_VERSION $d/lc2
  grep -q LC_MAIN $d/lc2

  # The unwinders of the 2013 releases look a function up in
  # __unwind_info alone, so the FDEs of the functions it covers go.
  # (Clang gives code for these targets compact unwind records alone,
  # so the object has both by hand.)
  if [ $ARCH = arm64 ]; then
    insn=ret encoding=0x04000000 ra=30 cfa='0x0c, 31, 0, 0, 0'
  else
    insn=ret encoding=0x02010000 ra=16 cfa='0x0c, 7, 8, 0x90, 1'
  fi
  cat <<EOF | $cc -o $d/e.o -c -xassembler -
.text
.globl _ef
.p2align 2
_ef:
  $insn
Lef_end:
.section __LD,__compact_unwind,regular,debug
.p2align 3
.quad _ef
.long Lef_end - _ef
.long $encoding
.quad 0
.quad 0
.section __TEXT,__eh_frame,coalesced,no_toc+strip_static_syms+live_support
.p2align 3
EH_CIE:
.long 0x14
.long 0
.byte 1
.asciz "zR"
.byte 1, 0x78, $ra, 1, 0x10
.byte $cfa, 0, 0
EH_FDE:
.long 0x1c
.long 0x1c
.quad _ef - .
.quad Lef_end - _ef
.byte 0, 0, 0, 0, 0, 0, 0, 0
.subsections_via_symbols
EOF
  read old new <<< "$(versions 2013fall)"
  if [ $old != - ]; then
    link $old exe12 $d/e.o 2> /dev/null
    sects $d/exe12 > $d/s12
    grep -q __TEXT,__eh_frame $d/s12
  fi
  link $new exe13 $d/e.o 2> /dev/null
  sects $d/exe13 > $d/s13
  not grep -q __TEXT,__eh_frame $d/s13

  # __DATA_CONST from the 2019 releases, with no gap at iOS 13.4 (as
  # macOS has at 10.15.4).
  read old new <<< "$(versions 2019fall)"
  if [ $old != - ]; then
    link $old exe3 2> /dev/null
    sects $d/exe3 > $d/s3
    grep -q __DATA,__const $d/s3
  fi
  link $new exe4 2> /dev/null
  sects $d/exe4 > $d/s4
  grep -q __DATA_CONST,__const $d/s4
  if [ $os != xros ]; then
    link 13.4 exe5 2> /dev/null
    sects $d/exe5 > $d/s5
    grep -q __DATA_CONST,__const $d/s5
  fi

  # Relative method lists from the 2020 releases (never in an x86-64
  # executable, so look at a dylib).
  dylib() {
    $mold -arch $ARCH -platform_version $name $1 27.0 -syslibroot $sdk -lSystem -lobjc \
      -dylib $d/objc.o -o $d/$2 2> /dev/null
  }
  read old new <<< "$(versions 2020fall)"
  if [ $old != - ]; then
    dylib $old lib1.dylib
    sects $d/lib1.dylib > $d/s6
    not grep -q __TEXT,__objc_methlist $d/s6
  fi
  dylib $new lib2.dylib
  sects $d/lib2.dylib > $d/s7
  grep -q __TEXT,__objc_methlist $d/s7

  # Chains of offsets from the image from the 2021 releases; a VM
  # address before them.
  read old new <<< "$(versions 2021fall)"
  if [ $old != - ]; then
    link $old exe6 -fixup_chains 2> /dev/null
    [ "$(fmt $d/exe6)" = 2 ]
  fi
  link $new exe7 -fixup_chains 2> /dev/null
  [ "$(fmt $d/exe7)" = 6 ]

  # Class and superclass references go read-only after fixups from the
  # 2024 spring releases, and fold into the GOT from the fall ones.
  read old new <<< "$(versions 2024spring)"
  dylib $old lib3.dylib
  sects $d/lib3.dylib > $d/s8
  grep -q __DATA,__objc_superrefs $d/s8
  dylib $new lib4.dylib
  sects $d/lib4.dylib > $d/s9
  grep -q __DATA_CONST,__objc_superrefs $d/s9
  grep -q __DATA_CONST,__objc_classrefs $d/s9
  read old new <<< "$(versions 2024fall)"
  dylib $new lib5.dylib
  sects $d/lib5.dylib > $d/s10
  grep -q __DATA_CONST,__objc_superrefs $d/s10
  not grep -q __objc_classrefs $d/s10

  # dyld delays a library's initializers from the 2024 fall releases
  # on, and loads a library lazily from the 2026 fall ones.
  link $old exe8 -delay_library $d/libl.dylib 2> $d/log8
  grep -q 'delay-init will be ignored' $d/log8
  link $new exe9 -delay_library $d/libl.dylib 2> $d/log9
  not grep -q 'delay-init will be ignored' $d/log9
  read old new <<< "$(versions 2026fall)"
  link $old exe10 -lazy_library $d/libl.dylib 2> $d/log10
  grep -q 'lazy-load will be ignored' $d/log10
  link $new exe11 -lazy_library $d/libl.dylib 2> $d/log11
  not grep -q 'lazy-load will be ignored' $d/log11
done

# Firmware, which no OS release governs, has the defaults of the sets
# up to 2021's: relative method lists, but class references that stay
# writable.
sdk=$(xcrun --show-sdk-path)
cat <<EOF2 | $CC -o $t/fw.o -c -xobjective-c -fno-objc-arc -
@interface Foo { void *isa; }
- (int)m;
@end
@implementation Foo
- (int)m { return 1; }
@end
@interface Bar : Foo
@end
@implementation Bar
- (int)m { return [super m]; }
@end
EOF2
$mold -arch $ARCH -platform_version firmware 1.0 1.0 -syslibroot $sdk -lobjc -dylib $t/fw.o \
  -o $t/fw.dylib 2> /dev/null
sects $t/fw.dylib > $t/fw.sects
grep -q __TEXT,__objc_methlist $t/fw.sects
grep -q __DATA,__objc_superrefs $t/fw.sects
