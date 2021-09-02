ifeq ($(origin CC), default)
  CC = clang
endif

ifeq ($(origin CXX), default)
  CXX = clang++
endif

GIT_HASH ?= $(shell [ -d .git ] && git rev-parse HEAD)

OS ?= $(shell uname -s)

CPPFLAGS = -g -pthread -std=c++20 -fPIE \
           -DMOLD_VERSION=\"0.9.4\" \
           -DGIT_HASH=\"$(GIT_HASH)\" \
	   $(EXTRA_CPPFLAGS)
LDFLAGS += $(EXTRA_LDFLAGS)
LIBS = -pthread -lz -lxxhash -ldl -lm

SRCS=$(wildcard elf/*.cc)
HEADERS=$(wildcard elf/*.h)
OBJS=$(SRCS:.cc=.o)

PREFIX ?= /usr
DEST = $(DESTDIR)$(PREFIX)
DEBUG ?= 0
LTO ?= 0
ASAN ?= 0
TSAN ?= 0

ifeq ($(DEBUG), 1)
  CPPFLAGS += -O0
else
  CPPFLAGS += -O2
endif

ifeq ($(LTO), 1)
  CPPFLAGS += -flto -O3
  LDFLAGS  += -flto
endif

ifeq ($(ASAN), 1)
  CPPFLAGS += -fsanitize=address
  LDFLAGS  += -fsanitize=address
else ifeq ($(TSAN), 1)
  CPPFLAGS += -fsanitize=thread
  LDFLAGS  += -fsanitize=thread
else ifneq ($(OS), Darwin)
  # By default, we want to use mimalloc as a memory allocator.
  # Since replacing the standard malloc is not compatible with ASAN,
  # we do that only when ASAN is not enabled.
  ifdef SYSTEM_MIMALLOC
    LIBS += -lmimalloc
  else
    MIMALLOC_LIB = out/mimalloc/libmimalloc.a
    CPPFLAGS += -Ithird-party/mimalloc/include
    LIBS += -Wl,-whole-archive $(MIMALLOC_LIB) -Wl,-no-whole-archive
  endif
endif

ifdef SYSTEM_TBB
  LIBS += -ltbb
else
  TBB_LIB = out/tbb/libs/libtbb.a
  LIBS += $(TBB_LIB)
  CPPFLAGS += -Ithird-party/tbb/include
endif

ifneq ($(OS), Darwin)
  LIBS += -lcrypto
endif

all: mold mold-wrapper.so

mold: $(OBJS) $(MIMALLOC_LIB) $(TBB_LIB)
	$(CXX) $(CXXFLAGS) $(OBJS) -o $@ $(LDFLAGS) $(LIBS)
	ln -sf mold ld

mold-wrapper.so: elf/mold-wrapper.c Makefile
	$(CC) -fPIC -shared -o $@ $< -ldl

$(OBJS): $(HEADERS) Makefile

$(MIMALLOC_LIB):
	mkdir -p out/mimalloc
	(cd out/mimalloc; CFLAGS=-DMI_USE_ENVIRON=0 cmake -G'Unix Makefiles' ../../third-party/mimalloc)
	$(MAKE) -C out/mimalloc mimalloc-static

$(TBB_LIB):
	mkdir -p out/tbb
	(cd out/tbb; cmake -G'Unix Makefiles' -DBUILD_SHARED_LIBS=OFF -DTBB_TEST=OFF -DCMAKE_CXX_FLAGS=-D__TBB_DYNAMIC_LOAD_ENABLED=0 -DTBB_STRICT=OFF ../../third-party/tbb)
	$(MAKE) -C out/tbb tbb
	(cd out/tbb; ln -sf *_relwithdebinfo libs)

test tests check: all
	 $(MAKE) -C test --output-sync --no-print-directory

install: all
	install -m 755 -d $(DEST)/bin
	install -m 755 mold $(DEST)/bin
	strip $(DEST)/bin/mold

	install -m 755 -d $(DEST)/lib/mold
	install -m 644 mold-wrapper.so $(DEST)/lib/mold
	strip $(DEST)/lib/mold/mold-wrapper.so

	install -m 755 -d $(DEST)/share/man/man1
	install -m 644 docs/mold.1 $(DEST)/share/man/man1
	rm -f $(DEST)/share/man/man1/mold.1.gz
	gzip -9 $(DEST)/share/man/man1/mold.1

uninstall:
	rm -f $(DEST)/bin/mold
	rm -f $(DEST)/share/man/man1/mold.1.gz
	rm -rf $(DEST)/lib/mold

clean:
	rm -rf *.o elf/*.o *~ mold mold-wrapper.so test/tmp out ld

.PHONY: all test tests check clean $(TESTS)
