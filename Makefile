CC = clang
CXX = clang++

MIMALLOC_LIB = mimalloc/out/release/libmimalloc.a

CPPFLAGS = -g -Imimalloc/include -pthread -std=c++20 \
           -Wno-deprecated-volatile -Wno-switch \
           -DGIT_HASH=\"$(shell git rev-parse HEAD)\" \
	   $(EXTRA_CPPFLAGS)
LDFLAGS = $(EXTRA_LDFLAGS)
LIBS = -Wl,-as-needed -lcrypto -pthread -ltbb -lz -lxxhash -ldl
OBJS = main.o object_file.o input_sections.o output_chunks.o \
       mapfile.o perf.o linker_script.o archive_file.o output_file.o \
       subprocess.o gc_sections.o icf.o symbols.o cmdline.o filepath.o \
       glob.o passes.o tar.o compress.o arch_x86_64.o arch_i386.o

PREFIX ?= /usr
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
else
  # By default, we want to use mimalloc as a memory allocator.
  # Since replacing the standard malloc is not compatible with ASAN,
  # we do that only when ASAN is not enabled.
  LDFLAGS += -Wl,-whole-archive $(MIMALLOC_LIB) -Wl,-no-whole-archive
endif

ifeq ($(TSAN), 1)
  CPPFLAGS += -fsanitize=thread
  LDFLAGS  += -fsanitize=thread
endif

all: mold mold-wrapper.so

mold: $(OBJS) $(MIMALLOC_LIB)
	$(CXX) $(CFLAGS) $(OBJS) -o $@ $(LDFLAGS) $(LIBS)

mold-wrapper.so: mold-wrapper.c Makefile
	$(CC) -fPIC -shared -o $@ $< -ldl

$(OBJS): mold.h elf.h Makefile

$(MIMALLOC_LIB):
	mkdir -p mimalloc/out/release
	(cd mimalloc/out/release; CFLAGS=-DMI_USE_ENVIRON=0 cmake ../..)
	$(MAKE) -C mimalloc/out/release

test: all
	for i in test/*.sh; do $$i || exit 1; done

install: all
	install -m 755 mold $(PREFIX)/bin
	strip $(PREFIX)/bin/mold

	install -m 755 -d $(PREFIX)/lib/mold
	install -m 644 mold-wrapper.so $(PREFIX)/lib/mold
	strip $(PREFIX)/lib/mold/mold-wrapper.so

	install -m 644 docs/mold.1 $(PREFIX)/share/man/man1
	rm -f $(PREFIX)/share/man/man1/mold.1.gz
	gzip -9 $(PREFIX)/share/man/man1/mold.1

uninstall:
	rm -rf $(PREFIX)/bin/mold $(PREFIX)/share/man/man1/mold.1.gz \
	       $(PREFIX)/lib/mold

clean:
	rm -f *.o *~ mold mold-wrapper.so

.PHONY: all test clean
