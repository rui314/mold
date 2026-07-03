#!/bin/sh -e

#-----------------------------------------------------------------------------
# Bundle release
#-----------------------------------------------------------------------------

OSARCH=""
OSNAME=""
MI_TAG=""
MI_COMMIT=""

#---------------------------------------------------------
# Helper functions
#---------------------------------------------------------

info() {
  if [ -z "$QUIET" ] ; then
    echo "$@"
  fi
}

warn() {
  echo "$@" >&2
}

stop() {
  warn $@
  exit 1
}

has_cmd() {
  command -v "$1" > /dev/null 2>&1
}


#---------------------------------------------------------
# Detect git tag and commit
#---------------------------------------------------------

detect_git_tag() {
  if [ -z "$MI_COMMIT" ] ; then
    MI_COMMIT=`git rev-parse HEAD`
  fi
  if [ -z "$MI_TAG" ] ; then
    MI_TAG=`git describe --tag`
  fi
  info "Tag        : $MI_TAG"
  info "Commit     : $MI_COMMIT"
}


#---------------------------------------------------------
# Detect OS and cpu architecture
#---------------------------------------------------------

detect_osarch() {
  arch="$(uname -m)"
  case "$arch" in
    x86_64*|amd64*)
      arch="x64";;
    x86*|i[35678]86*)
      arch="x86";;
    arm64*|aarch64*|armv8*)
      arch="arm64";;
    arm*)
      arch="arm";;
    parisc*)
      arch="hppa";;
  esac

  OSNAME="linux"
  case "$(uname)" in
    [Ll]inux)
      OSNAME="linux";;
    [Dd]arwin)
      OSNAME="macos";;
    [Ff]ree[Bb][Ss][Dd])
      OSNAME="unix-freebsd";;
    [Oo]pen[Bb][Ss][Dd])
      OSNAME="unix-openbsd";;
    *)
      info "Warning: assuming generic Linux"
  esac
  OSARCH="$OSNAME-$arch"
}

#---------------------------------------------------------
# Command line options
#---------------------------------------------------------

process_options() {
  while : ; do
    flag="$1"
    case "$flag" in
    *=*)  flag_arg="${flag#*=}";;
    *)    flag_arg="yes" ;;
    esac
    # echo "option: $flag, arg: $flag_arg"
    case "$flag" in
      "") break;;
      -q|--quiet)
          QUIET="yes";;
      -p) shift
          PREFIX="$1";;
      -p=*|--prefix=*)
          PREFIX=`eval echo $flag_arg`;;  # no quotes so ~ gets expanded (issue #412)
      -h|--help|-\?|help|\?)
          MODE="help";;
      *) case "$flag" in
           *) warn "warning: unknown option \"$1\".";;
         esac;;
    esac
    shift
  done
}

#---------------------------------------------------------
# Download
#---------------------------------------------------------

download_failed() { # <program> <url>
  stop "unable to download: $2"
}

download_file() {  # <url|file> <destination file>
  case "$1" in
    ftp://*|http://*|https://*)
      info "Downloading: $1"
      if has_cmd curl ; then
        if ! curl ${QUIET:+-sS} --proto =https --tlsv1.2 -f -L -o "$2" "$1"; then
          download_failed "curl" $1
        fi
      elif has_cmd wget ; then
        if ! wget ${QUIET:+-q} --https-only "-O$2" "$1"; then
          download_failed "wget" $1
        fi
      else
        stop "Neither 'curl' nor 'wget' is available; install one to continue."
      fi;;
    *)
      info "Copying: $1"
      if ! cp $1 $2 ; then
        stop "Unable to copy from $1"
      fi;;
  esac
}

#---------------------------------------------------------
# Bundle
#---------------------------------------------------------

download_source_at_commit() { # <commit> <output file>
  download_file "https://github.com/microsoft/mimalloc/archive/$1.tar.gz" "$2"
}

build_test_install() { # <type> <bundledir> <prefix> <cmake args>
  info "Build, test, install: $1 to $3"
  build_dir="$2/$1"
  mkdir -p "$build_dir"
  cmake . -B "$build_dir" $4
  cmake --build "$build_dir" --parallel 4
  ctest --test-dir "$build_dir"
  cmake --install "$build_dir" --prefix "$3"
}

main_bundle() {
  # config
  bundle_dir="out/bundle"
  mkdir -p "$bundle_dir"
  if [ -z "$PREFIX" ] ; then
    prefix_dir="$bundle_dir/prefix"
  else
    prefix_dir="$PREFIX"
  fi

  # build
  build_test_install "debug"   "$bundle_dir" "$prefix_dir" "-DCMAKE_BUILD_TYPE=Debug"
  build_test_install "release" "$bundle_dir" "$prefix_dir" "-DCMAKE_BUILD_TYPE=Release -DMI_OPT_ARCH=ON"
  build_test_install "secure"  "$bundle_dir" "$prefix_dir" "-DCMAKE_BUILD_TYPE=Release -DMI_OPT_ARCH=ON -DMI_SECURE=ON"

  # archive binaries
  binary_archive_name="mimalloc-$MI_TAG-$OSARCH.tar.gz"
  binary_archive="$bundle_dir/$binary_archive_name"
  info "Create binary archive: $binary_archive_name"
  (cd "$prefix_dir" && tar -czvf "../$binary_archive_name" .)

  # source archive
  if [ "$OSNAME" = "linux" ] ; then
    info "Download source archive for $MI_TAG"
    source_archive="$bundle_dir/mimalloc-$MI_TAG-source.tar.gz"
    download_source_at_commit "$MI_COMMIT" "$source_archive"
  fi

  # done
  info ""
  info "Created:"
  info "  - $binary_archive"
  if [ -n "$source_archive" ] ; then
    info "  - $source_archive"
  fi
  info ""
  info "Done."
}

#---------------------------------------------------------
# Main
#---------------------------------------------------------

main_help() {
  info "command:"
  info "  ./bin/bundle.sh [options]"
  info ""
  info "options:"
  info "  -q, --quiet              suppress output"
  info "  -p, --prefix=<dir>       prefix directory ($PREFIX)"
  info "  -h, --help               show command line options"
  info ""
}

main_start() {
  detect_osarch
  detect_git_tag
  process_options $@
  if [ "$MODE" = "help" ] ; then
    main_help
  else
    main_bundle
  fi
}

# note: only start executing commands now to guard against partial downloads
main_start $@
