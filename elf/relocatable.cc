#include "mold.h"

#include <tbb/parallel_for_each.h>

namespace mold::elf {

template <typename E>
static void r_create_synthetic_sections(Context<E> &ctx) {
  auto push = [&]<typename T>(T *x) {
    ctx.chunks.push_back(x);
    ctx.chunk_pool.emplace_back(x);
    return x;
  };

  ctx.ehdr = push(new OutputEhdr<E>(0));
  ctx.shdr = push(new OutputShdr<E>);
  ctx.eh_frame = push(new EhFrameSection<E>);
  ctx.eh_frame_reloc = push(new EhFrameRelocSection<E>);
  ctx.strtab = push(new StrtabSection<E>);
  ctx.symtab = push(new SymtabSection<E>);
  ctx.shstrtab = push(new ShstrtabSection<E>);
}

template <typename E>
static void create_comdat_group_sections(Context<E> &ctx) {
  for (ObjectFile<E> *file : ctx.objs) {
    for (ComdatGroupRef<E> &ref : file->comdat_groups) {
      if (ref.group->owner != file->priority)
        continue;

      Symbol<E> *sym = file->symbols[file->elf_sections[ref.sect_idx].sh_info];
      assert(sym);

      std::vector<Chunk<E> *> members;

      for (u32 i : ref.members) {
        const ElfShdr<E> &shdr = file->elf_sections[i];

        if (shdr.sh_type == (is_rela<E> ? SHT_RELA : SHT_REL)) {
          InputSection<E> *isec = file->sections[shdr.sh_info].get();
          assert(isec);
          assert(isec->output_section);
          assert(isec->output_section->reloc_sec);
          members.push_back(isec->output_section->reloc_sec);
          continue;
        }

        InputSection<E> *isec = file->sections[i].get();
        assert(isec);
        assert(isec->is_alive);
        assert(isec->output_section);
        members.push_back(isec->output_section);
      }

      ComdatGroupSection<E> *sec = new ComdatGroupSection<E>(*sym, members);
      ctx.chunks.push_back(sec);
      ctx.chunk_pool.emplace_back(sec);
    }
  }
}

template <typename E>
static void r_claim_unresolved_symbols(Context<E> &ctx) {
  Timer t(ctx, "r_claim_unresolved_symbols");

  tbb::parallel_for_each(ctx.objs, [&](ObjectFile<E> *file) {
    if (!file->is_alive)
      return;

    for (i64 i = file->first_global; i < file->elf_syms.size(); i++) {
      const ElfSym<E> &esym = file->elf_syms[i];
      Symbol<E> &sym = *file->symbols[i];
      if (!esym.is_undef())
        continue;

      std::scoped_lock lock(sym.mu);

      if (sym.file &&
          (!sym.esym().is_undef() || sym.file->priority <= file->priority))
        continue;

      sym.file = file;
      sym.origin = 0;
      sym.value = 0;
      sym.sym_idx = i;
    }
  });
}

template <typename E>
static u64 r_set_osec_offsets(Context<E> &ctx) {
  u64 offset = 0;
  for (Chunk<E> *chunk : ctx.chunks) {
    offset = align_to(offset, chunk->shdr.sh_addralign);
    chunk->shdr.sh_offset = offset;
    offset += chunk->shdr.sh_size;
  }
  return offset;
}

template <typename E>
void combine_objects(Context<E> &ctx) {
  compute_merged_section_sizes(ctx);

  bin_sections(ctx);

  append(ctx.chunks, collect_output_sections(ctx));

  r_create_synthetic_sections(ctx);

  r_claim_unresolved_symbols(ctx);

  compute_section_sizes(ctx);

  sort_output_sections(ctx);

  create_output_symtab(ctx);

  ctx.eh_frame->construct(ctx);

  create_reloc_sections(ctx);

  create_comdat_group_sections(ctx);

  compute_section_headers(ctx);

  i64 filesize = r_set_osec_offsets(ctx);
  ctx.output_file =
    OutputFile<Context<E>>::open(ctx, ctx.arg.output, filesize, 0777);
  ctx.buf = ctx.output_file->buf;

  copy_chunks(ctx);
  clear_padding(ctx);
  ctx.output_file->close(ctx);

  if (ctx.arg.print_map)
    print_map(ctx);
}

using E = MOLD_TARGET;

template void combine_objects(Context<E> &);

} // namespace mold::elf
