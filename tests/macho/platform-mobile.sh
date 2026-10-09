#!/bin/bash
source "$(dirname "$0")"/common.inc

# Besides macOS and firmware, mold links for iOS, tvOS and visionOS, on
# devices and in their simulators. -platform_version names each by its
# OS's name (visionOS by its development name, xros, too), with
# "-simulator" after it for the simulator, in any case, or by its
# number in LC_BUILD_VERSION; -target takes the triples clang makes;
# without either, the first object names the platform. A device's SDK
# has arm64 libraries only, a simulator's x86_64 ones too.
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
bv() {
  otool -l $1 | grep -A4 'cmd LC_BUILD_VERSION' |
    awk '$1 == "platform" || $1 == "minos" || $1 == "sdk" { printf "%s %s ", $1, $2 }'
}

for p in ios:2:17.0 ios-simulator:7:17.0 tvos:3:17.0 tvos-simulator:8:17.0 \
  xros:11:2.0 xros-simulator:12:2.0; do
  IFS=: read name num ver <<< "$p"
  [ $ARCH = arm64 ] || [[ $name = *-simulator ]] || continue
  sdk=$(xcrun --sdk $(sdk_name $name) --show-sdk-path 2> /dev/null) || continue
  os=${name%-simulator}
  env=${name#$os}
  triple=$ARCH-apple-$os$ver$env

  cat <<EOF | cc -target $triple -isysroot $sdk -o $t/$name.o -c -xassembler -
.text
.globl _main
_main:
  ret
EOF
  link() { $mold -arch $ARCH -syslibroot $sdk -lSystem $t/$name.o -o $t/$name "$@"; }

  for spelling in $name $(echo $name | tr a-z A-Z) $num; do
    link -platform_version $spelling $ver 27.0
    [ "$(bv $t/$name)" = "platform $num minos $ver sdk 27.0 " ]
  done
  if [ $os = xros ]; then
    link -platform_version visionos$env $ver 27.0
    [ "$(bv $t/$name)" = "platform $num minos $ver sdk 27.0 " ]
  fi

  $mold -target $triple -syslibroot $sdk -lSystem $t/$name.o -o $t/$name
  [ "$(bv $t/$name)" = "platform $num minos $ver sdk n/a " ]

  link
  bv $t/$name | grep -q "^platform $num minos $ver "
done

# A device's code doesn't link into a simulator's image, nor the other
# way around, and the two platforms can't both be named.
if [ $ARCH = arm64 ] && sdk=$(xcrun --sdk iphonesimulator --show-sdk-path 2> /dev/null); then
  not $mold -arch $ARCH -platform_version ios-simulator 17.0 27.0 -syslibroot $sdk -lSystem \
    $t/ios.o -o $t/exe1 2> $t/log1
  grep -q "building for 'iOS-simulator', but linking in object file (.*ios.o) built for 'iOS'" \
    $t/log1
  not $mold -arch $ARCH -platform_version ios 17.0 27.0 -platform_version ios-simulator \
    17.0 27.0 -syslibroot $sdk -lSystem $t/ios-simulator.o -o $t/exe2 2> $t/log2
  grep -q 'incompatible platforms: iOS - iOS-simulator' $t/log2
fi

# mold doesn't link for watchOS (whose devices run arm64_32 code) or
# Mac Catalyst.
if is_mold; then
  for name in watchos watchos-simulator mac-catalyst 4 6; do
    not $mold -arch $ARCH -platform_version $name 10.0 11.0 -o $t/exe3 2> $t/log3
    grep -q "platform: $name$" $t/log3
  done
fi
