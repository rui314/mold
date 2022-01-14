PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
LIBDIR ?= $(PREFIX)/lib
LIBEXECDIR ?= $(PREFIX)/libexec
MANDIR ?= $(PREFIX)/share/man

D = $(DESTDIR)

# CXX defaults to `g++`. Rewrite it with a vendor-neutral compiler
# name `c++`.
ifeq ($(origin CXX), default)
  CXX = c++
endif

# If you want to keep symbols in the installed binary, run make with
# `STRIP=true` to run /bin/true instead of the strip command.
STRIP ?= strip

OS   ?= $(shell uname -s)
ARCH ?= $(shell uname -m)

ifneq ($(findstring -android,$(shell $(CC) -dumpmachine)),)
  OS = Android
endif

# Used for both C and C++
COMMON_FLAGS = -pthread -fPIE -fno-unwind-tables -fno-asynchronous-unwind-tables

CFLAGS ?= -O2
CFLAGS += $(COMMON_FLAGS) $(EXTRA_CFLAGS)

CXXFLAGS ?= -O2
CXXFLAGS += $(COMMON_FLAGS) $(EXTRA_CXXFLAGS) -std=c++20 -fno-exceptions
CPPFLAGS += -DMOLD_VERSION=\"1.0.1\" -DLIBDIR="\"$(LIBDIR)\""
LIBS = -pthread -lz -ldl -lm

SRCS=$(wildcard *.cc elf/*.cc macho/*.cc)
HEADERS=$(wildcard *.h elf/*.h macho/*.h)
OBJS=$(SRCS:%.cc=out/%.o)

DEBUG ?= 0
LTO ?= 0
ASAN ?= 0
TSAN ?= 0

GIT_HASH ?= $(shell [ -d .git ] && git rev-parse HEAD)
ifneq ($(GIT_HASH),)
  CPPFLAGS += -DGIT_HASH=\"$(GIT_HASH)\"
endif

ifeq ($(DEBUG), 1)
  CXXFLAGS += -O0 -g
endif

ifeq ($(LTO), 1)
  CXXFLAGS += -flto -O3
  LDFLAGS  += -flto
endif

# By default, we want to use mimalloc as a memory allocator. mimalloc
# is disabled when ASAN or TSAN is on, as they are not compatible.
# It's also disabled on macOS and Android because it didn't work on
# those hosts.
USE_MIMALLOC = 1
ifeq ($(OS), Darwin)
  USE_MIMALLOC = 0
else ifeq ($(OS), Android)
  USE_MIMALLOC = 0
else ifeq ($(ASAN), 1)
  USE_MIMALLOC = 0
else ifeq ($(TSAN), 1)
  USE_MIMALLOC = 0
endif

ifeq ($(USE_MIMALLOC), 1)
  ifdef SYSTEM_MIMALLOC
    LIBS += -lmimalloc
  else
    MIMALLOC_LIB = out/mimalloc/libmimalloc.a
    CPPFLAGS += -Ithird-party/mimalloc/include
    LIBS += -Wl,-whole-archive $(MIMALLOC_LIB) -Wl,-no-whole-archive
  endif
endif

ifeq ($(ASAN), 1)
  CXXFLAGS += -fsanitize=address
  LDFLAGS  += -fsanitize=address
endif

ifeq ($(TSAN), 1)
  CXXFLAGS += -fsanitize=thread
  LDFLAGS  += -fsanitize=thread
endif

ifdef SYSTEM_TBB
  LIBS += -ltbb
else
  TBB_LIB = out/tbb/libs/libtbb.a
  LIBS += $(TBB_LIB)
  CPPFLAGS += -Ithird-party/tbb/include
endif

ifdef SYSTEM_XXHASH
  LIBS += -lxxhash
else
  XXHASH_LIB = third-party/xxhash/libxxhash.a
  LIBS += $(XXHASH_LIB)
  CPPFLAGS += -Ithird-party/xxhash
endif

ifeq ($(OS), Linux)
  # glibc before 2.17 need librt for clock_gettime
  LIBS += -lrt
endif

# Use pkg-config to know where libcrypto resides.
ifneq ($(OS), Darwin)
  CPPFLAGS += $(shell pkg-config --cflags-only-I openssl)
  LIBS     += $(shell pkg-config --libs-only-L   openssl) -lcrypto
endif

# '-latomic' flag is needed building on riscv64 system
# RV32 system doesn't tested yet
# seems like '-atomic' would be better but not working.
ifeq ($(ARCH), riscv64)
  LIBS += -latomic
endif

# -Wc++11-narrowing is a fatal error on Android, so disable it.
ifeq ($(OS), Android)
  CXXFLAGS += -Wno-c++11-narrowing
endif

all: mold mold-wrapper.so

mold: $(OBJS) $(MIMALLOC_LIB) $(TBB_LIB) $(XXHASH_LIB)
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) $(LDFLAGS) $(EXTRA_LDFLAGS) $(OBJS) -o $@ $(LIBS)
	ln -sf mold ld
	ln -sf mold ld64.mold

mold-wrapper.so: elf/mold-wrapper.c Makefile
	$(CC) $(CPPFLAGS) $(CFLAGS) -fPIC -shared -o $@ $(LDFLAGS) $< -ldl

out/%.o: %.cc $(HEADERS) Makefile out/elf/.keep out/macho/.keep
	$(CXX) $(CPPFLAGS) $(CXXFLAGS) -c -o $@ $<

out/elf/.keep:
	mkdir -p out/elf
	touch $@

out/macho/.keep:
	mkdir -p out/macho
	touch $@

$(MIMALLOC_LIB):
	mkdir -p out/mimalloc
	(cd out/mimalloc; CFLAGS=-DMI_USE_ENVIRON=0 cmake -G'Unix Makefiles' ../../third-party/mimalloc)
	$(MAKE) -C out/mimalloc mimalloc-static

$(TBB_LIB):
	mkdir -p out/tbb
	(cd out/tbb; cmake -G'Unix Makefiles' -DBUILD_SHARED_LIBS=OFF -DTBB_TEST=OFF -DCMAKE_CXX_FLAGS=-D__TBB_DYNAMIC_LOAD_ENABLED=0 -DTBB_STRICT=OFF ../../third-party/tbb)
	$(MAKE) -C out/tbb tbb
	(cd out/tbb; ln -sf *_relwithdebinfo libs)

$(XXHASH_LIB):
	$(MAKE) -C third-party/xxhash libxxhash.a

test tests check: all
ifeq ($(OS), Darwin)
	$(MAKE) -C test -f Makefile.darwin --no-print-directory
else
	$(MAKE) -C test -f Makefile.linux --no-print-directory --output-sync
endif

	@if test -t 1; then \
	  printf '\e[32mPassed all tests\e[0m\n'; \
	else \
	  echo 'Passed all tests'; \
	fi

install: all
	install -m 755 -d $D$(BINDIR)
	install -m 755 mold $D$(BINDIR)
	$(STRIP) $D$(BINDIR)/mold

	install -m 755 -d $D$(LIBDIR)/mold
	install -m 644 mold-wrapper.so $D$(LIBDIR)/mold
	$(STRIP) $D$(LIBDIR)/mold/mold-wrapper.so

	install -m 755 -d $D$(LIBEXECDIR)/mold
	ln -sf $(BINDIR)/mold $D$(LIBEXECDIR)/mold/ld

	install -m 755 -d $D$(MANDIR)/man1
	install -m 644 docs/mold.1 $D$(MANDIR)/man1

	ln -sf mold $D$(BINDIR)/ld.mold
	ln -sf mold $D$(BINDIR)/ld64.mold

uninstall:
	rm -f $D$(BINDIR)/mold $D$(BINDIR)/ld.mold $D$(BINDIR)/ld64.mold
	rm -f $D$(MANDIR)/man1/mold.1
	rm -rf $D$(LIBDIR)/mold

clean:
	rm -rf *~ mold mold-wrapper.so out ld ld64.mold
	$(MAKE) -C third-party/xxhash clean

.PHONY: all test tests check clean
