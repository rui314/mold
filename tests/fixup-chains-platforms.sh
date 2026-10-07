#!/bin/bash
source "$(dirname "$0")"/common.inc

# Chained fixups are the default from each OS's own version, not a
# season's: iOS 13.4, whose dyld first read them, tvOS 14, the
# simulators 15 on either architecture (macOS's later start for x86-64
# executables doesn't apply), and every visionOS. ld-prime's "new OS
# versions" start there too: __mod_init_func becomes __init_offsets,
# and -no_pie is deprecated.
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

for name in $plats; do
  sdk=$(xcrun --sdk $(sdk_name $name) --show-sdk-path 2> /dev/null) || continue
  os=${name%-simulator}
  env=${name#$os}
  d=$t/$name
  mkdir -p $d
  case $name in
  ios) old=13.3 new=13.4 ;;
  tvos) old=13.4 new=14.0 ;;
  *-simulator) old=14.6 new=15.0 ;;
  esac
  [ $os = xros ] && old=- new=1.0

  cat <<EOF | cc -target $ARCH-apple-${os}1.0$env -isysroot $sdk -o $d/a.o -c -xc -
int g = 1;
int *gp = &g;
__attribute__((constructor)) static void init(void) { g = 0; }
int main() { return *gp; }
EOF
  link() {
    $mold -arch $ARCH -platform_version $name $1 27.0 -syslibroot $sdk -lSystem $d/a.o \
      -o $d/$2 "${@:3}" 2> /dev/null
  }
  check() {
    otool -l $d/$1 > $d/$1.lc
    if [ $2 = chained ]; then
      grep -q LC_DYLD_CHAINED_FIXUPS $d/$1.lc
      grep -q 'sectname __init_offsets' $d/$1.lc
    else
      grep -q LC_DYLD_INFO_ONLY $d/$1.lc
      grep -q 'sectname __mod_init_func' $d/$1.lc
    fi
  }

  if [ $old != - ]; then
    link $old exe1
    check exe1 classic
    link $old lib1.dylib -dylib
    check lib1.dylib classic
    $mold -arch $ARCH -platform_version $name $old 27.0 -syslibroot $sdk -lSystem $d/a.o \
      -o $d/exe2 -no_pie 2> $d/log2
    not grep -q 'no_pie is deprecated' $d/log2
  fi
  link $new exe3
  check exe3 chained
  link $new lib3.dylib -dylib
  check lib3.dylib chained
  link $new bundle3 -bundle
  check bundle3 chained
  $mold -arch $ARCH -platform_version $name $new 27.0 -syslibroot $sdk -lSystem $d/a.o \
    -o $d/exe4 -no_pie 2> $d/log4
  grep -q -- '-no_pie is deprecated when targeting new OS versions' $d/log4
done
