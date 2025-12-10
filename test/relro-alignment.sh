#!/bin/bash
. $(dirname $0)/common.inc

[ $MACHINE = i686 ] && skip
[ $MACHINE = arm ] && skip
[ $MACHINE = armeb ] && skip
[ $MACHINE = ppc ] && skip
[ $MACHINE = sh4 ] && skip
[ $MACHINE = sh4aeb ] && skip
[ $MACHINE = m68k ] && skip

cat <<EOF | $CC -c -xc -o $t/a.o -
#include <stdio.h>
#include <unistd.h>
#include <stdint.h>

#define PT_GNU_RELRO 0x6474e552

typedef struct {
  char e_ident[16];
  uint16_t e_type;
  uint16_t e_machine;
  uint32_t e_version;
  uint64_t e_entry;
  uint64_t e_phoff;
  uint64_t e_shoff;
  uint32_t e_flags;
  uint16_t e_ehsize;
  uint16_t e_phentsize;
  uint16_t e_phnum;
  uint16_t e_shentsize;
  uint16_t e_shnum;
  uint16_t e_shstrndx;
} Ehdr;

typedef struct {
  uint32_t p_type;
  uint32_t p_flags;
  uint64_t p_offset;
  uint64_t p_vaddr;
  uint64_t p_paddr;
  uint64_t p_filesz;
  uint64_t p_memsz;
  uint64_t p_align;
} Phdr;

extern char __ehdr_start[];

int main() {
  Ehdr *ehdr = (Ehdr *)__ehdr_start;
  Phdr *phdr = (Phdr *)(__ehdr_start + ehdr->e_phoff);
  int pagesz = sysconf(_SC_PAGESIZE);

  for (int i = 0; i < ehdr->e_phnum; i++) {
    if (phdr[i].p_type == PT_GNU_RELRO) {
      if ((phdr[i].p_vaddr + phdr[i].p_memsz) % pagesz == 0) {
        printf("Aligned: vaddr=%lx memsz=%lx pagesize=%x\n",
               phdr[i].p_vaddr, phdr[i].p_memsz, pagesz);
      } else {
        printf("Unaligned: vaddr=%lx memsz=%lx pagesize=%x\n",
               phdr[i].p_vaddr, phdr[i].p_memsz, pagesz);
      }
      return 0;
    }
  }
  printf("PT_GNU_RELRO missing\n");
}
EOF

$CC -B. -o $t/exe1 $t/a.o -Wl,-z,relro
$QEMU $t/exe1 | grep Aligned

$CC -B. -o $t/exe2 $t/a.o -Wl,-z,relro,-z,separate-loadable-segments
$QEMU $t/exe2 | grep Aligned
