VERSION = 1.5.1

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

SRCS = compress.cc demangle.cc elf/arch-arm32.cc elf/arch-arm64.cc elf/arch-i386.cc elf/arch-ppc64v1.cc elf/arch-ppc64v2.cc elf/arch-riscv.cc elf/arch-s390x.cc elf/arch-sparc64.cc elf/arch-x86-64.cc filepath.cc glob.cc hyperloglog.cc macho/arch-arm64.cc macho/arch-x86-64.cc macho/yaml.cc main.cc multi-glob.cc perf.cc tar.cc uuid.cc

ELF_TARGETS = X86_64 I386 ARM64 ARM32 RV32LE RV32BE RV64LE RV64BE PPC64V1 PPC64V2 S390X SPARC64
MACHO_TARGETS = X86_64 ARM64

ELF_TEMPLATES = elf/cmdline.cc elf/dwarf.cc elf/gc-sections.cc elf/icf.cc elf/input-files.cc elf/input-sections.cc elf/linker-script.cc elf/lto.cc elf/main.cc elf/mapfile.cc elf/output-chunks.cc elf/passes.cc elf/relocatable.cc elf/subprocess.cc elf/thunks.cc

MACHO_TEMPLATES = macho/cmdline.cc macho/dead-strip.cc macho/input-files.cc macho/input-sections.cc macho/lto.cc macho/main.cc macho/mapfile.cc macho/output-chunks.cc macho/tapi.cc

ELF_SRCS = $(foreach src, $(ELF_TEMPLATES), $(foreach arch, $(ELF_TARGETS), $(src).$(arch).cc))

MACHO_SRCS = $(foreach src, $(MACHO_TEMPLATES), $(foreach arch, $(MACHO_TARGETS), $(src).$(arch).cc))

OBJS = $(SRCS:%.cc=out/objs/%.o) $(ELF_SRCS:%.cc=out/objs2/%.o) $(MACHO_SRCS:%.cc=out/objs2/%.o) out/objs/rust-demangle.o out/objs2/git-hash.o

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

DEPFLAGS = -MT $@ -MMD -MP -MF out/d/$*.d

all: prebuild
	$(MAKE) mold mold-wrapper.so

-include $(SRCS:%.cc=d/%.d)

prebuild:
	@echo 'WARNING: Use of `make` is deprecated. Please use `cmake` to build mold.'
	sleep 10
	mkdir -p out/d out/d/elf out/d/macho out/srcs/elf out/srcs/macho out/objs/elf out/objs/macho out/objs2/elf out/objs2/macho out/objs2/src/elf out/objs2/src/macho

out/srcs/git-hash.cc: FORCE
	cmake -DSOURCE_DIR=. -DOUTPUT_FILE=out/srcs/git-hash.cc -P update-git-hash.cmake

FORCE:

mold: $(OBJS) $(MIMALLOC_LIB) $(TBB_LIB) $(ZSTD_LIB)
	$(CXX) $(OBJS) -o $@ $(MOLD_LDFLAGS) $(LDFLAGS)
	ln -sf mold ld
	ln -sf mold ld64

mold-wrapper.so: elf/mold-wrapper.c
	$(CC) $(DEPFLAGS) $(CFLAGS) -fPIC -shared -o $@ $< $(MOLD_WRAPPER_LDFLAGS) $(LDFLAGS)

out/objs/rust-demangle.o: third-party/rust-demangle/rust-demangle.c
	$(CC) $(CFLAGS) -c -o $@ $<

define gen_target
out/srcs/$1.$2.cc: $1
	@echo "#define MOLD_$2 1" > out/srcs/$1.$2.cc
	@echo "#define MOLD_TARGET $2" >> out/srcs/$1.$2.cc
	@echo "#include \"../../../$1\"" >> out/srcs/$1.$2.cc
endef

$(foreach src, $(ELF_TEMPLATES), $(foreach arch, $(ELF_TARGETS), $(eval $(call gen_target,$(src),$(arch)))))

$(foreach src, $(MACHO_TEMPLATES), $(foreach arch, $(MACHO_TARGETS), $(eval $(call gen_target,$(src),$(arch)))))

out/objs/%.o: %.cc
	$(CXX) $(MOLD_CXXFLAGS) $(DEPFLAGS) $(CXXFLAGS) -c -o $@ $<

out/objs2/%.o: out/srcs/%.cc
	$(CXX) $(MOLD_CXXFLAGS) $(DEPFLAGS) $(CXXFLAGS) -c -o $@ $<

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
	$(MAKE) test

test-all: all
	$(MAKE) test-arch TRIPLE=x86_64-linux-gnu
	$(MAKE) test-arch TRIPLE=i686-linux-gnu
	$(MAKE) test-arch TRIPLE=aarch64-linux-gnu
	$(MAKE) test-arch TRIPLE=arm-linux-gnueabihf
	$(MAKE) test-arch TRIPLE=riscv64-linux-gnu

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

.PHONY: all prebuild test tests check clean test-arch test-all test-asan test-ubsan test-tsan $(TESTS)
