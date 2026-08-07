#!/bin/bash
#
# This test script takes a Gentoo package name and tries to build it
# using mold in a Podman environment. We chose Gentoo Linux as a test
# target, because its source-based packages allow us to build programs
# locally and run their test suites without any hassle.
#
# We first build a package with mold acting as /usr/bin/ld. Only if
# that fails, we build the same package again with GNU ld to tell
# whether the failure is mold's fault. The package's own test suite
# runs in both cases; FEATURES=test is scoped to just the package
# under test via package.env, because applying it to the entire
# dependency graph makes a large fraction of the tree unresolvable.
# Results are classified into three directories:
#
#   success: the package built fine with mold (or, for a control
#            build, with GNU ld)
#   failure: the package built with GNU ld but not with mold, which
#            means it hit a mold bug
#   broken:  the package didn't build even with GNU ld, so the
#            failure is not mold's fault
#
# You can get a complete list of Gentoo packages available for testing
# with the following command:
#
# podman run --rm mold-gentoo emerge --color n -s '' | \
#   perl -ne 'next unless m!^\*\s+(\S+/\S+)!; print "$1\n"'

package="$1"

if [ "$package" = "" ]; then
  echo "Usage: $0 gentoo-package-name"
  exit 1
fi

set -x

# Test results are keyed by the commit hash embedded in the mold
# binary, so refuse to run with a binary that doesn't have one.
git_hash=$(./dist/mold --version | perl -ne '/\(([0-9a-f]{40})\b/ and print $1;')

if [ "$git_hash" = "" ]; then
  echo "Cannot get a git hash from ./dist/mold --version. Run this script at the mold source root, with dist/mold built from a git checkout."
  exit 1
fi

cache_dir=/var/cache/mold-gentoo

# Create a Podman image. The lock prevents parallel invocations of
# this script from racing to build the same image.
#
# A few notes on the container configuration:
#
# - We accept ~amd64 (testing) packages to test the newest version of
#   everything. The stage3 image is built for the stable profile, so
#   we update the whole system to ~amd64 once here; without that,
#   half-upgraded packages (e.g. perl and its virtuals) cause
#   unsolvable dependency conflicts in every container.
#
# - EMERGE_DEFAULT_OPTS makes emerge automatically apply the USE flag
#   adjustments it would otherwise just print and give up on. An
#   ephemeral container has no /etc/portage/package.use accumulated by
#   an administrator, so without this, thousands of packages are
#   unbuildable out of the box.
#
# - freetype and harfbuzz mutually depend on each other, so we
#   pre-install them with the documented two-step bootstrap. During
#   that step, pillow is built without truetype support to break
#   another dependency cycle (docutils -> pillow -> harfbuzz -> glib
#   -> docutils). The override is removed afterwards; once the cycle
#   participants are installed, they satisfy buildtime dependencies
#   and the cycle can no longer form.
#
# - qtbase's test suite requires icu (REQUIRED_USE), which autounmask
#   doesn't resolve on its own, hence the package.use entry.
#
# - gcc doesn't auto-select a newly installed major version, so we
#   switch to the newest slot explicitly after the world update.
#
# - lib/gentoo-gpkg-workaround.py patches a deadlock in portage's
#   binary package extraction. See that file for details.
#
# - static-libs is not in USE because it contradicts REQUIRED_USE
#   constraints in some packages (e.g. lvm2 forbids static-libs with
#   udev), and building static archives doesn't exercise the linker
#   anyway. Where a dependency genuinely needs it, emerge re-enables
#   it per package via autounmask.
#
# - /etc/portage/env/test.conf allows marking a single package for
#   FEATURES=test through package.env, which is how we scope test
#   suites to just the package under test.
#
# - The image build compiles a couple hundred packages from source,
#   because the Gentoo binhost only carries stable versions built with
#   default USE flags, which rarely match our ~amd64/USE combination.
#   To pay that cost only once, the build publishes what it compiles
#   to the GNU ld binary package pool and reuses it, so a rebuilt
#   image only compiles packages whose version has changed since the
#   last build. The toolchain step still uses the binhost, where our
#   configuration does match (e.g. clang). Since it verifies binhost
#   package signatures and our own pool is unsigned, it runs with a
#   private PKGDIR so the two never mix.
(
  flock 9
  if ! podman image exists mold-gentoo; then
    cat <<EOF | podman build -v $cache_dir/distfiles:/var/cache/distfiles -v $cache_dir/ccache:/ccache -v $cache_dir/binpkgs-ld:/var/cache/binpkgs -v `pwd`/lib:/mold-lib:ro -t mold-gentoo - || exit 1
FROM docker.io/gentoo/stage3
RUN emerge-webrsync
RUN echo 'USE="X ssl elogind -systemd truetype jpeg jpeg2k tiff zstd -perl udev lcms alsa"' >> /etc/portage/make.conf && \
    echo 'ACCEPT_KEYWORDS="~amd64"' >> /etc/portage/make.conf && \
    echo 'ACCEPT_LICENSE="* -@EULA"' >> /etc/portage/make.conf && \
    echo 'FEATURES="nostrip ccache binpkg-multi-instance -ipc-sandbox -network-sandbox -pid-sandbox -sandbox"' >> /etc/portage/make.conf && \
    echo 'CCACHE_DIR="/ccache"' >> /etc/portage/make.conf && \
    echo 'MAKEOPTS="-j$(nproc) --load-average=100"' >> /etc/portage/make.conf && \
    echo 'EMERGE_DEFAULT_OPTS="--autounmask-continue=y --autounmask-license=n --autounmask-backtrack=y --backtrack=100 --jobs=16 --load-average=100"' >> /etc/portage/make.conf && \
    mkdir -p /etc/portage/env /etc/portage/package.use && \
    echo 'FEATURES="test"' > /etc/portage/env/test.conf && \
    printf 'media-libs/freetype harfbuzz\ndev-qt/qtbase icu\n' > /etc/portage/package.use/mold && \
    echo 'dev-python/pillow -truetype' > /etc/portage/package.use/cycle-break && \
    FEATURES=buildpkg emerge --usepkg --update --deep --newuse @world && \
    gcc-config \$(gcc-config -l | tail -n 1 | awk '{print \$2}') && env-update && \
    USE=-harfbuzz FEATURES=buildpkg emerge --oneshot media-libs/freetype && \
    FEATURES=buildpkg emerge --usepkg --oneshot media-libs/freetype media-libs/harfbuzz && \
    rm /etc/portage/package.use/cycle-break && \
    PKGDIR=/var/tmp/binpkgs FEATURES='getbinpkg binpkg-request-signature' emerge gdb lld llvm-core/clang vim emacs strace ccache dev-build/cmake dev-vcs/git && \
    rm -rf /var/tmp/portage /var/tmp/binpkgs
RUN python3 /mold-lib/gentoo-gpkg-workaround.py
EOF
  fi
) 9> /tmp/mold-gentoo-image.lock || exit 1

if [ "$package" = dev-libs/concurrencykit ]; then
  echo "Skipping known broken package: $package"
  exit 0
fi

# Build a given package in Podman.
#
# Build artifacts are shared between containers through host
# directories bind-mounted into every container:
#
#   /var/cache/mold-gentoo/ccache: compiler cache
#   /var/cache/mold-gentoo/distfiles: source tarballs, so that each
#     distfile is downloaded only once across all containers and runs
#   /var/cache/mold-gentoo/binpkgs-{mold,ld}: binary packages, so that each
#     dependency is built only once instead of once per container that
#     needs it
#
# Every emerge runs with FEATURES=buildpkg to publish what it built to
# the binary package pool, and dependencies are installed with
# --usepkg from the pool when an exact version and USE match exists.
# The package under test itself is always built from source. In the
# mold run, dependencies are still linked with mold and their binaries
# are still exercised by the test suites of everything built on top of
# them; the pool just memoizes that work across containers. The mold
# and GNU ld runs use separate pools so that a control build can never
# install mold-linked binaries.
cmd1='(cd /usr/bin; ln -sf /mold/dist/mold $(realpath ld))'
cmd2="echo '$package test.conf' > /etc/portage/package.env"
cmd3="FEATURES=buildpkg emerge --usepkg --onlydeps $package"
cmd4="FEATURES=buildpkg emerge $package"
filename=`echo "$package" | sed 's!/!_!g'`
podman="podman run --rm --pids-limit=-1 --cap-add=SYS_PTRACE -v `pwd`:/mold:ro -v $cache_dir/ccache:/ccache -v $cache_dir/distfiles:/var/cache/distfiles"
run="mold-gentoo timeout -v -k 15s 3h chrt --idle 0 nice -n 19 bash -c"
dir=gentoo/$git_hash

mkdir -p "$dir"/success "$dir"/failure "$dir"/broken

# Build the package with mold. If this succeeds, we don't need a
# control build.
$podman -v $cache_dir/binpkgs-mold:/var/cache/binpkgs $run "$cmd1 && $cmd2 && $cmd3 && $cmd4" >& "$dir"/"$filename".mold
if [ $? = 0 ]; then
  mv "$dir"/"$filename".mold "$dir"/success
  exit 0
fi

# Build the package again with GNU ld to tell whether the failure is
# mold's fault.
$podman -v $cache_dir/binpkgs-ld:/var/cache/binpkgs $run "$cmd2 && $cmd3 && $cmd4" >& "$dir"/"$filename".ld
if [ $? = 0 ]; then
  mv "$dir"/"$filename".mold "$dir"/failure
  mv "$dir"/"$filename".ld "$dir"/success
else
  mv "$dir"/"$filename".mold "$dir"/broken
  mv "$dir"/"$filename".ld "$dir"/broken
fi
