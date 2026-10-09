#!/bin/bash
source "$(dirname "$0")"/common.inc

# Before iOS and tvOS 12, which brought LC_BUILD_VERSION, an image
# names its deployment target with the legacy LC_VERSION_MIN_IPHONEOS
# or LC_VERSION_MIN_TVOS {version, sdk}, -r output too. Those commands
# name no simulator: an x86-64 image's is the simulator's, so an arm64
# simulator image gets LC_BUILD_VERSION at any version, and so does a
# visionOS one.
plats='ios-simulator tvos-simulator xros-simulator'
[ $ARCH = arm64 ] && plats="ios tvos $plats"

sdk_name() {
  case $1 in
  ios) echo iphoneos ;;
  ios-simulator) echo iphonesimulator ;;
  tvos) echo appletvos ;;
  tvos-simulator) echo appletvsimulator ;;
  xros-simulator) echo xrsimulator ;;
  esac
}
cmd() {
  otool -l $1 | grep -A4 -E 'cmd LC_(BUILD_VERSION|VERSION_MIN)' |
    awk '$1 == "cmd" || $1 == "version" || $1 == "minos" || $1 == "sdk" { printf "%s %s ", $1, $2 }'
}

for name in $plats; do
  sdk=$(xcrun --sdk $(sdk_name $name) --show-sdk-path 2> /dev/null) || continue
  os=${name%-simulator}
  env=${name#$os}
  d=$t/$name
  mkdir -p $d
  echo 'int main() { return 0; }' |
    cc -target $ARCH-apple-${os}1.0$env -isysroot $sdk -o $d/a.o -c -xc -
  link() {
    $mold -arch $ARCH -platform_version $name $1 27.0 -syslibroot $sdk -lSystem $d/a.o \
      -o $d/$2 "${@:3}" 2> /dev/null
  }

  case $os in
  ios) legacy=LC_VERSION_MIN_IPHONEOS ;;
  tvos) legacy=LC_VERSION_MIN_TVOS ;;
  *) legacy= ;;
  esac
  [ $ARCH = arm64 ] && [ $name != $os ] && legacy=

  link 11.4 exe1
  link 12.0 exe2
  $mold -arch $ARCH -platform_version $name 11.4 27.0 -r $d/a.o -o $d/r.o 2> /dev/null
  if [ -n "$legacy" ]; then
    [ "$(cmd $d/exe1)" = "cmd $legacy version 11.4 sdk 27.0 " ]
    [ "$(cmd $d/r.o)" = "cmd $legacy version 11.4 sdk 27.0 " ]
  else
    [ "$(cmd $d/exe1)" = 'cmd LC_BUILD_VERSION minos 11.4 sdk 27.0 ' ]
    [ "$(cmd $d/r.o)" = 'cmd LC_BUILD_VERSION minos 11.4 sdk 27.0 ' ]
  fi
  [ "$(cmd $d/exe2)" = 'cmd LC_BUILD_VERSION minos 12.0 sdk 27.0 ' ]
done

# Unlike macOS's, tvOS's version 0.0 has one too.
if [ $ARCH = arm64 ] && sdk=$(xcrun --sdk appletvos --show-sdk-path 2> /dev/null); then
  $mold -arch $ARCH -platform_version tvos 0.0 27.0 -syslibroot $sdk -lSystem \
    $t/tvos/a.o -o $t/exe3 2> /dev/null
  [ "$(cmd $t/exe3)" = 'cmd LC_VERSION_MIN_TVOS version 0.0 sdk 27.0 ' ]
fi
