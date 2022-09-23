#include "mold.h"

namespace mold::macho {

template <typename E>
OutputSection<E> &get_output_section(Context<E> &ctx, const MachSection &hdr) {
  static std::unordered_set<std::string_view> set = {
    "__got", "__auth_got", "__auth_ptr", "__nl_symbol_ptr", "__const",
    "__cfstring", "__mod_init_func", "__mod_term_func", "__objc_classlist",
    "__objc_nlclslist", "__objc_catlist", "__objc_nlcatlist", "__objc_protolist",
  };

  std::string_view seg = hdr.get_segname();
  std::string_view sect = hdr.get_sectname();

  if (seg == "__DATA" && set.contains(sect)) {
    seg = "__DATA_CONST";
  } else if (seg == "__TEXT" && sect == "__StaticInit") {
    sect = "__text";
  }

  return *OutputSection<E>::get_instance(ctx, seg, sect);
}

template <typename E>
InputSection<E>::InputSection(Context<E> &ctx, ObjectFile<E> &file,
                              const MachSection &hdr, u32 secidx)
  : file(file), hdr(hdr), secidx(secidx), osec(get_output_section(ctx, hdr)) {
  if (hdr.type != S_ZEROFILL)
    contents = file.mf->get_contents().substr(hdr.offset, hdr.size);
}

template <typename E>
void InputSection<E>::parse_relocations(Context<E> &ctx) {
  // Parse mach-o relocations to fill `rels` vector
  rels = read_relocations(ctx, file, hdr);

  // Sort `rels` vector
  sort(rels, [](const Relocation<E> &a, const Relocation<E> &b) {
    return a.offset < b.offset;
  });

  // Find subsections this relocation section refers to
  auto cmp = [](Subsection<E> *subsec, u32 addr) {
    return subsec->input_addr < addr;
  };

  auto begin = std::lower_bound(file.subsections.begin(),
                                file.subsections.end(), hdr.addr, cmp);
  auto end = std::lower_bound(begin, file.subsections.end(),
                              hdr.addr + hdr.size, cmp);

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

using E = MOLD_TARGET;

template class InputSection<E>;

} // namespace mold::macho
