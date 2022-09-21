VERSION = 1.4.2

PREFIX = /usr/local
BINDIR = $(PREFIX)/bin
LIBDIR = $(PREFIX)/lib
LIBEXECDIR = $(PREFIX)/libexec
MANDIR = $(PREFIX)/share/man
DOCDIR = $(PREFIX)/share/doc

INSTALL = install
INSTALL_PROGRAM = $(INSTALL)
INSTALL_DATA = $(INSTALL) -m 644

OS := $(shell uname -s)
ARCH := $(shell uname -m)

ifeq ($(OS), Darwin)
  TESTS := $(wildcard test/macho/*.sh)
else
  TESTS := $(wildcard test/elf/*.sh)
endif

D = $(DESTDIR)

# CXX defaults to `g++`. Rewrite it with a vendor-neutral compiler
# name `c++`.
ifeq ($(origin CXX), default)
  CXX = c++
endif

# If you want to keep symbols in the installed binary, run make with
# `STRIP=true` to run /bin/true instead of the strip command.
STRIP = strip

SRCS = $(wildcard *.cc elf/*.cc macho/*.cc)
OBJS = $(SRCS:%.cc=out/%.o) out/rust-demangle.o out/git-hash.o

IS_ANDROID = 0
ifneq ($(findstring -android,$(shell $(CC) -dumpmachine)),)
  IS_ANDROID = 1
endif

# If you want to compile mold for debugging, invoke make as
# `make CXXFLAGS=-g`.
CFLAGS = -O2
CXXFLAGS = -O2

MOLD_CXXFLAGS := -std=c++20 -fno-exceptions -fno-unwind-tables \
                 -fno-asynchronous-unwind-tables \
                 -Wno-sign-compare -Wno-unused-function \
                 -DMOLD_VERSION=\"$(VERSION)\" -DLIBDIR="\"$(LIBDIR)\""

ifeq ($(OS), OpenBSD)
  MOLD_LDFLAGS := -pthread -lz -lm
else
  MOLD_LDFLAGS := -pthread -lz -lm -ldl
endif

LTO = 0
ifeq ($(LTO), 1)
  CXXFLAGS += -flto -O3
  LDFLAGS  += -flto
endif

# By default, we want to use mimalloc as a memory allocator. mimalloc
# is disabled on macOS and Android because it didn't work on those hosts.
USE_MIMALLOC = 1
ifeq ($(OS), Darwin)
  USE_MIMALLOC = 0
else ifeq ($(ARCH), i686)
  USE_MIMALLOC = 0
else ifeq ($(IS_ANDROID), 1)
  USE_MIMALLOC = 0
endif

ifeq ($(USE_MIMALLOC), 1)
  ifdef SYSTEM_MIMALLOC
    MOLD_CXXFLAGS += -DUSE_SYSTEM_MIMALLOC
    MOLD_LDFLAGS += -lmimalloc
  else
    MIMALLOC_LIB = out/mimalloc/libmimalloc.a
    MOLD_CXXFLAGS += -Ithird-party/mimalloc/include
    MOLD_LDFLAGS += -Wl,-whole-archive $(MIMALLOC_LIB) -Wl,-no-whole-archive
  endif
endif

# Note: Do NOT specify `SYSTEM_TBB=1` unless your system-wide OneTBB
# library is compiled with https://github.com/oneapi-src/oneTBB/pull/824.
# mold with an unpatched OneTBB is unstable when doing LTO if the system
# is under high load.
ifdef SYSTEM_TBB
  MOLD_LDFLAGS += -ltbb
else
  TBB_LIB = out/tbb/libs/libtbb.a
  MOLD_LDFLAGS += $(TBB_LIB)
  MOLD_CXXFLAGS += -Ithird-party/tbb/include
endif

ifdef SYSTEM_ZSTD
  MOLD_LDFLAGS += -lzstd
else
  ZSTD_LIB = out/zstd/lib/libzstd.a
  MOLD_CXXFLAGS += -Ithird-party/zstd/lib
  MOLD_LDFLAGS += $(ZSTD_LIB)
endif

ifeq ($(OS), Linux)
  ifeq ($(IS_ANDROID), 0)
    # glibc before 2.17 need librt for clock_gettime
    MOLD_LDFLAGS += -Wl,-push-state -Wl,-as-needed -lrt -Wl,-pop-state
  endif
endif

NEEDS_LIBCRYPTO = 1
ifeq ($(OS), Darwin)
  NEEDS_LIBCRYPTO = 0
endif

ifeq ($(NEEDS_LIBCRYPTO), 1)
  MOLD_LDFLAGS += -lcrypto
endif

# '-latomic' flag is needed building on armv6/riscv64 systems.
# Seems like '-atomic' would be better but not working.
ifneq (,$(filter armv6% riscv64, $(ARCH)))
  MOLD_LDFLAGS += -latomic
endif

# -Wc++11-narrowing is a fatal error on Android, so disable it.
ifeq ($(IS_ANDROID), 1)
  MOLD_CXXFLAGS += -Wno-c++11-narrowing
endif

ifeq ($(OS), Linux)
  MOLD_WRAPPER_LDFLAGS = -Wl,-push-state -Wl,-no-as-needed -ldl -Wl,-pop-state
endif

DEPFLAGS = -MT $@ -MMD -MP -MF out/$*.d

all: mold mold-wrapper.so

-include $(SRCS:%.cc=out/%.d)

out/git-hash.cc: FORCE
	cmake -DSOURCE_DIR=. -DOUTPUT_FILE=out/git-hash.cc -P update-git-hash.cmake

FORCE:

out/git-hash.o: out/git-hash.cc
	$(CXX) $(MOLD_CXXFLAGS) $(CXXFLAGS) -c -o $@ $<

mold: $(OBJS) $(MIMALLOC_LIB) $(TBB_LIB) $(ZSTD_LIB)
	$(CXX) $(OBJS) -o $@ $(MOLD_LDFLAGS) $(LDFLAGS)
	ln -sf mold ld
	ln -sf mold ld64

mold-wrapper.so: elf/mold-wrapper.c
	$(CC) $(DEPFLAGS) $(CFLAGS) -fPIC -shared -o $@ $< $(MOLD_WRAPPER_LDFLAGS) $(LDFLAGS)

out/rust-demangle.o: third-party/rust-demangle/rust-demangle.c
	$(CC) $(CFLAGS) -c -o $@ $<

out/%.o: %.cc out/elf/.keep out/macho/.keep
	$(CXX) $(MOLD_CXXFLAGS) $(DEPFLAGS) $(CXXFLAGS) -c -o $@ $<

out/elf/.keep out/macho/.keep:
	mkdir -p $(@D)
	touch $@

$(MIMALLOC_LIB):
	mkdir -p out/mimalloc
	(cd out/mimalloc; CFLAGS=-DMI_USE_ENVIRON=0 cmake -G'Unix Makefiles' ../../third-party/mimalloc)
	$(MAKE) -C out/mimalloc mimalloc-static

$(TBB_LIB):
	mkdir -p out/tbb
	(cd out/tbb; cmake -G'Unix Makefiles' -DCMAKE_BUILD_TYPE=RelWithDebInfo -DBUILD_SHARED_LIBS=OFF -DTBB_TEST=OFF -DCMAKE_CXX_FLAGS="$(CXXFLAGS) -D__TBB_DYNAMIC_LOAD_ENABLED=0" -DTBB_STRICT=OFF ../../third-party/tbb)
	$(MAKE) -C out/tbb tbb
	(cd out/tbb; ln -sf *_relwithdebinfo libs)

$(ZSTD_LIB):
	mkdir -p out/zstd
	(cd out/zstd; cmake -G'Unix Makefiles' ../../third-party/zstd/build/cmake -DZSTD_BUILD_STATIC=ON -DZSTD_BUILD_SHARED=OFF -DZSTD_BUILD_PROGRAMS=OFF -DZSTD_MULTITHREAD_SUPPORT=OFF -DZSTD_BUILD_TESTS=OFF)
	$(MAKE) -C out/zstd libzstd_static

test tests check: all
ifeq ($(OS), Darwin)
	@$(MAKE) $(TESTS) --no-print-directory
else
	@$(MAKE) $(TESTS) --no-print-directory --output-sync
endif

	@if test -t 1; then \
	  printf '\e[32mPassed all tests\e[0m\n'; \
	else \
	  echo 'Passed all tests'; \
	fi

test-arch:
	TEST_CC=${TRIPLE}-gcc \
	TEST_CXX=${TRIPLE}-g++ \
	TEST_GCC=${TRIPLE}-gcc \
	TEST_GXX=${TRIPLE}-g++ \
	OBJDUMP=${TRIPLE}-objdump \
	MACHINE=${MACHINE} \
	QEMU="qemu-${MACHINE} -L /usr/${TRIPLE}" \
	$(MAKE) test

test-all: all
	$(MAKE) test-arch TRIPLE=x86_64-linux-gnu MACHINE=x86_64
	$(MAKE) test-arch TRIPLE=i686-linux-gnu MACHINE=i386
	$(MAKE) test-arch TRIPLE=aarch64-linux-gnu MACHINE=aarch64
	$(MAKE) test-arch TRIPLE=arm-linux-gnueabihf MACHINE=arm
	$(MAKE) test-arch TRIPLE=riscv64-linux-gnu MACHINE=riscv64

# macOS's GNU make hasn't been updated since 3.8.1 perhaps due a concern
# of GPLv3. The --output-sync flag was introduced in GNU Make 4.0, so we
# can't use that flag on macOS.
#
# `tail -r | tail -r` is a poor-man's way to enable full buffering on a
# command output. `tail -r` outputs an input from the last line to the
# first.
$(TESTS):
ifeq ($(OS), Darwin)
	@set -o pipefail; ./$@ 2>&1 | tail -r | tail -r
else
	@./$@
endif

install: all
	$(INSTALL) -d $D$(BINDIR)
	$(INSTALL_PROGRAM) mold $D$(BINDIR)
	$(STRIP) $D$(BINDIR)/mold

	$(INSTALL) -d $D$(LIBDIR)/mold

ifneq ($(OS), Darwin)
	$(INSTALL_DATA) mold-wrapper.so $D$(LIBDIR)/mold
	$(STRIP) $D$(LIBDIR)/mold/mold-wrapper.so
endif

	$(INSTALL) -d $D$(LIBEXECDIR)/mold
	cmake -DSOURCE=$D$(BINDIR)/mold -DDEST=$D$(LIBEXECDIR)/mold/ld -P create-symlink.cmake

	$(INSTALL) -d $D$(MANDIR)/man1
	$(INSTALL_DATA) docs/mold.1 $D$(MANDIR)/man1
	ln -sf mold.1 $D$(MANDIR)/man1/ld.mold.1

	$(INSTALL) -d $D$(DOCDIR)/mold
	$(INSTALL_DATA) LICENSE $D$(DOCDIR)/mold/

	ln -sf mold $D$(BINDIR)/ld.mold
	ln -sf mold $D$(BINDIR)/ld64.mold

uninstall:
	rm -f $D$(BINDIR)/mold $D$(BINDIR)/ld.mold $D$(BINDIR)/ld64.mold
	rm -f $D$(MANDIR)/man1/mold.1
	rm -rf $D$(LIBDIR)/mold

test-asan test-ubsan:
	$(MAKE) USE_MIMALLOC=0 CXXFLAGS='-fsanitize=address -fsanitize=undefined -O0 -g' LDFLAGS='-fsanitize=address -fsanitize=undefined' test

test-tsan:
	$(MAKE) USE_MIMALLOC=0 CXXFLAGS='-fsanitize=thread -O0 -g' LDFLAGS=-fsanitize=thread test

clean:
	rm -rf *~ mold mold-wrapper.so out ld ld64 mold-*-linux.tar.gz

.PHONY: all test tests check clean test-arch test-all test-asan test-ubsan test-tsan $(TESTS)
