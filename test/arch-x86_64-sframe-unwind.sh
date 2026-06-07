#!/bin/bash
. $(dirname $0)/common.inc

# Verify that the .sframe section mold produces is not just well-formed but
# actually usable for stack unwinding. We link a program that, using only
# its own merged .sframe (read via libsframe), walks a known call chain and
# checks that each recovered frame maps to the expected function.

# Skip unless the assembler emits SFrame Version 3.
cat <<EOF | $CC -Wa,--gsframe -o $t/probe.o -c -xc - 2>/dev/null || skip
int main() { return 0; }
EOF
readelf --sframe=.sframe $t/probe.o 2>/dev/null | grep -q SFRAME_VERSION_3 || skip

# Skip unless libsframe and its header are available.
echo 'int main(){return 0;}' | \
  $CC -o $t/probe -xc - -lsframe -include sframe-api.h 2>/dev/null || skip

cat <<'EOF' | $CC -O0 -g -fno-omit-frame-pointer -Wa,--gsframe -B. -o $t/exe -xc - -lsframe
#define _GNU_SOURCE
#include <sframe-api.h>
#include <link.h>
#include <stdint.h>
#include <stdio.h>

#ifndef PT_GNU_SFRAME
#define PT_GNU_SFRAME 0x6474e554
#endif

static uintptr_t sec_addr;
static size_t sec_size;

static int find_cb(struct dl_phdr_info *info, size_t sz, void *u) {
  (void)sz; (void)u;
  for (int i = 0; i < info->dlpi_phnum; i++) {
    const ElfW(Phdr) *p = &info->dlpi_phdr[i];
    if (p->p_type == PT_GNU_SFRAME) {
      sec_addr = info->dlpi_addr + p->p_vaddr;
      sec_size = p->p_memsz;
      return 1;
    }
  }
  return 0;
}

static sframe_decoder_ctx *dctx;
static int32_t fixed_ra;

#define MAXF 4096
static uintptr_t fstart[MAXF], fend[MAXF];
static int nfde;

static int find_fde(uintptr_t pc) {
  for (int i = 0; i < nfde; i++)
    if (fstart[i] <= pc && pc < fend[i])
      return i;
  return -1;
}

// One SFrame unwind step per loop iteration; records the start address of
// the function each frame belongs to.
static int unwind(uintptr_t pc, uintptr_t sp, uintptr_t fp,
                  uintptr_t *out, int max) {
  int n = 0;
  while (n < max) {
    int idx = find_fde(pc);
    if (idx < 0)
      break;
    out[n++] = fstart[idx];

    uint32_t num_fres, func_size;
    int64_t spo;
    unsigned char fi, fi2;
    uint8_t rep;
    if (sframe_decoder_get_funcdesc_v3(dctx, idx, &num_fres, &func_size,
                                       &spo, &fi, &fi2, &rep))
      break;
    uint32_t fde_type = SFRAME_V3_FDE_TYPE(fi2);

    int64_t off = pc - fstart[idx];
    sframe_frame_row_entry fre, cur;
    int have = 0;
    for (uint32_t k = 0; k < num_fres; k++) {
      if (sframe_decoder_get_fre(dctx, idx, k, &cur))
        break;
      if ((int64_t)cur.fre_start_addr <= off) { fre = cur; have = 1; }
      else break;
    }
    if (!have)
      break;

    int e = 0;
    int base = sframe_fre_get_base_reg_id(&fre, &e);
    int32_t cfa_off = sframe_fre_get_cfa_offset(dctx, &fre, fde_type, &e);
    uintptr_t cfa = (base == SFRAME_BASE_REG_SP ? sp : fp) + cfa_off;
    uintptr_t ra = *(uintptr_t *)(cfa + fixed_ra);

    e = 0;
    int32_t fp_off = sframe_fre_get_fp_offset(dctx, &fre, fde_type, &e);
    uintptr_t next_fp = (e == 0) ? *(uintptr_t *)(cfa + fp_off) : fp;

    if (!ra)
      break;
    sp = cfa;
    fp = next_fp;
    pc = ra;
  }
  return n;
}

static uintptr_t chain[16];
static int chain_n;

__attribute__((noinline)) static int level3(int x) {
  uintptr_t pc, sp, fp;
  asm volatile("leaq 1f(%%rip), %0\n1:\n\t"
               "movq %%rsp, %1\n\t"
               "movq %%rbp, %2"
               : "=r"(pc), "=r"(sp), "=r"(fp));
  chain_n = unwind(pc, sp, fp, chain, 16);
  return x + 1;
}

__attribute__((noinline)) static int level2(int x) { return level3(x) + x; }
__attribute__((noinline)) static int level1(int x) { return level2(x) + x; }

int main(void) {
  if (!dl_iterate_phdr(find_cb, NULL) || !sec_addr)
    return 1;

  int err = 0;
  dctx = sframe_decode((const char *)sec_addr, sec_size, &err);
  if (!dctx)
    return 1;
  fixed_ra = sframe_decoder_get_fixed_ra_offset(dctx);

  uint32_t num = sframe_decoder_get_num_fidx(dctx);
  for (uint32_t i = 0; i < num && nfde < MAXF; i++) {
    uint32_t num_fres, func_size;
    int64_t spo;
    unsigned char fi, fi2;
    uint8_t rep;
    if (sframe_decoder_get_funcdesc_v3(dctx, i, &num_fres, &func_size,
                                       &spo, &fi, &fi2, &rep))
      continue;
    int e = 0;
    uint32_t fo = sframe_decoder_get_offsetof_fde_start_addr(dctx, i, &e);
    fstart[nfde] = sec_addr + fo + spo; // PCREL: relative to the field
    fend[nfde] = fstart[nfde] + func_size;
    nfde++;
  }

  level1(0);

  uintptr_t want[] = { (uintptr_t)&level3, (uintptr_t)&level2,
                       (uintptr_t)&level1, (uintptr_t)&main };
  int n = sizeof(want) / sizeof(want[0]);
  if (chain_n < n)
    return 1;
  for (int i = 0; i < n; i++)
    if (chain[i] != want[i])
      return 1;
  return 0;
}
EOF

$QEMU $t/exe
