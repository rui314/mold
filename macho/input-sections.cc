#include "mold.h"

namespace mold::macho {

template <typename E>
InputSection<E>::InputSection(Context<E> &ctx, ObjectFile<E> &file,
                              const MachSection &hdr)
  : file(file), hdr(hdr),
    osec(*OutputSection<E>::get_instance(ctx, hdr.get_segname(),
                                         hdr.get_sectname())) {
  if (hdr.type != S_ZEROFILL)
    contents = file.mf->get_contents().substr(hdr.offset, hdr.size);
}

template <typename E>
void InputSection<E>::parse_relocations(Context<E> &ctx) {
  rels.reserve(hdr.nreloc);

  // Parse mach-o relocations to fill `rels` vector
  MachRel *rel = (MachRel *)(file.mf->data + hdr.reloff);
  for (i64 i = 0; i < hdr.nreloc; i++)
    rels.push_back(read_reloc(ctx, file, hdr, rel[i]));

  // Sort `rels` vector
  sort(rels, [](const Relocation<E> &a, const Relocation<E> &b) {
    return a.offset < b.offset;
  });

  // Find subsections for this section
  auto begin = std::lower_bound(
      file.subsections.begin(), file.subsections.end(), hdr.addr,
      [](std::unique_ptr<Subsection<E>> &subsec, u32 addr) {
    return subsec->input_addr < addr;
  });

  auto end = std::lower_bound(
      begin, file.subsections.end(), hdr.addr + hdr.size,
      [](std::unique_ptr<Subsection<E>> &subsec, u32 addr) {
    return subsec->input_addr < addr;
  });

  // Assign each subsection a group of relocations
  i64 i = 0;
  for (auto it = begin; it < end; it++) {
    Subsection<E> &subsec = **it;
    subsec.rel_offset = i;
    while (i < rels.size() &&
           rels[i].offset < subsec.input_offset + subsec.input_size) {
      rels[i].offset -= subsec.input_offset;
      i++;
    }
    subsec.nrels = i - subsec.rel_offset;
  }
}

#define INSTANTIATE(E)                          \
  template class InputSection<E>

INSTANTIATE(ARM64);
INSTANTIATE(X86_64);

} // namespace mold::macho
