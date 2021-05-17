CC=clang
CXX=clang++

CURRENT_DIR=$(shell pwd)
TBB_LIBDIR=$(wildcard $(CURRENT_DIR)/oneTBB/build/linux_intel64_*_release/)
MALLOC_LIBDIR=$(CURRENT_DIR)/mimalloc/out/release

CPPFLAGS=-g -IoneTBB/include -IxxHash -pthread -std=c++20 \
         -Wno-deprecated-volatile -Wno-switch \
         -DGIT_HASH=\"$(shell git rev-parse HEAD)\"
LDFLAGS=-L$(TBB_LIBDIR) -Wl,-rpath=$(TBB_LIBDIR) \
        -L$(MALLOC_LIBDIR) -Wl,-rpath=$(MALLOC_LIBDIR) \
        -L$(CURRENT_DIR)/xxHash -Wl,-rpath=$(CURRENT_DIR)/xxHash
LIBS=-lcrypto -pthread -ltbb -lmimalloc -lz -lxxhash -ldl
OBJS=main.o object_file.o input_sections.o output_chunks.o mapfile.o perf.o \
     linker_script.o archive_file.o output_file.o subprocess.o gc_sections.o \
     icf.o symbols.o cmdline.o filepath.o glob.o passes.o tar.o compress.o \
     arch_x86_64.o arch_i386.o

DEBUG ?= 0
ifeq ($(DEBUG), 1)
  CPPFLAGS += -O0
else
  CPPFLAGS += -O2
endif

STATIC ?= 0
ifeq ($(STATIC), 1)
  LDFLAGS += -static
endif

LTO ?= 0
ifeq ($(LTO), 1)
  CPPFLAGS += -flto -O3
  LDFLAGS  += -flto
endif

ASAN ?= 0
ifeq ($(ASAN), 1)
  CPPFLAGS += -fsanitize=address
  LDFLAGS  += -fsanitize=address
endif

TSAN ?= 0
ifeq ($(TSAN), 1)
  CPPFLAGS += -fsanitize=thread
  LDFLAGS  += -fsanitize=thread
endif

all: mold mold-wrapper.so

mold: $(OBJS)
	$(CXX) $(CFLAGS) $(OBJS) -o $@ $(LDFLAGS) $(LIBS)

mold-wrapper.so: mold-wrapper.c Makefile
	cc -fPIC -shared -o $@ $< -ldl

$(OBJS): mold.h elf.h Makefile

submodules:
	$(MAKE) -C oneTBB
	$(MAKE) -C oneTBB extra_inc=big_iron.inc
	mkdir -p mimalloc/out/release
	(cd mimalloc/out/release; CFLAGS=-DMI_USE_ENVIRON=0 cmake ../..)
	$(MAKE) -C mimalloc/out/release
	$(MAKE) -C xxHash

test: all
	for i in test/*.sh; do $$i || exit 1; done

install: all
	install -m 755 mold /usr/bin
	install -m 755 -d /usr/lib/mold
	install -m 644 mold-wrapper.so /usr/lib/mold
	install -m 644 docs/mold.1 /usr/share/man/man1
	gzip -9 /usr/share/man/man1/mold.1

clean:
	rm -f *.o *~ mold mold-wrapper.so

.PHONY: all intel_tbb test clean
