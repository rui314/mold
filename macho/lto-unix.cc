#include "lto.h"
#include "mold.h"

#include <algorithm>
#include <cstdio>
#include <dlfcn.h>
#include <unistd.h>

namespace mold::macho {

template <typename E>
static void do_load_plugin(Context<E> &ctx) {
  void *handle = dlopen(ctx.arg.lto_library.c_str(), RTLD_NOW | RTLD_GLOBAL);
  if (!handle)
    Fatal(ctx) << "could not open plugin file: " << dlerror();

  ctx.lto.dlopen_handle = handle;

  // I don't want to use a macro, but I couldn't find an easy way to
  // write the same thing without a macro.
#define DLSYM(x) ctx.lto.x = (decltype(ctx.lto.x))dlsym(handle, "lto_" #x)
  DLSYM(get_version);
  DLSYM(get_error_message);
  DLSYM(module_is_object_file);
  DLSYM(module_is_object_file_for_target);
  DLSYM(module_has_objc_category);
  DLSYM(module_is_object_file_in_memory);
  DLSYM(module_is_object_file_in_memory_for_target);
  DLSYM(module_create);
  DLSYM(module_create_from_memory);
  DLSYM(module_create_from_memory_with_path);
  DLSYM(module_create_in_local_context);
  DLSYM(module_create_in_codegen_context);
  DLSYM(module_create_from_fd);
  DLSYM(module_create_from_fd_at_offset);
  DLSYM(module_dispose);
  DLSYM(module_get_target_triple);
  DLSYM(module_set_target_triple);
  DLSYM(module_get_num_symbols);
  DLSYM(module_get_symbol_name);
  DLSYM(module_get_symbol_attribute);
  DLSYM(module_get_linkeropts);
  DLSYM(module_get_macho_cputype);
  DLSYM(module_has_ctor_dtor);
  DLSYM(codegen_set_diagnostic_handler);
  DLSYM(codegen_create);
  DLSYM(codegen_create_in_local_context);
  DLSYM(codegen_dispose);
  DLSYM(codegen_add_module);
  DLSYM(codegen_set_module);
  DLSYM(codegen_set_debug_model);
  DLSYM(codegen_set_pic_model);
  DLSYM(codegen_set_cpu);
  DLSYM(codegen_set_assembler_path);
  DLSYM(codegen_set_assembler_args);
  DLSYM(codegen_add_must_preserve_symbol);
  DLSYM(codegen_write_merged_modules);
  DLSYM(codegen_compile);
  DLSYM(codegen_compile_to_file);
  DLSYM(codegen_optimize);
  DLSYM(codegen_compile_optimized);
  DLSYM(api_version);
  DLSYM(set_debug_options);
  DLSYM(codegen_debug_options);
  DLSYM(codegen_debug_options_array);
  DLSYM(initialize_disassembler);
  DLSYM(codegen_set_should_internalize);
  DLSYM(codegen_set_should_embed_uselists);
#undef DLSYM
}

template <typename E>
void load_lto_plugin(Context<E> &ctx) {
  std::call_once(ctx.lto_plugin_loaded, [&] { do_load_plugin(ctx); });
}

template <typename E>
void do_lto(Context<E> &ctx) {
  LTOCodeGen *cg = ctx.lto.codegen_create();

  // Add bitcode files to CodeGen.
  for (const std::string &opt : ctx.arg.mllvm)
    ctx.lto.codegen_debug_options(cg, opt.c_str());

  for (ObjectFile<E> *file : ctx.objs)
    if (file->lto_module)
      ctx.lto.codegen_add_module(cg, file->lto_module);

  // Mark symbols that have to be preserved. All symbols that are not
  // marked here may be internalized and deleted as an extenrally-
  // visible symbol.
  if (ctx.output_type == MH_DYLIB || ctx.arg.export_dynamic) {
    for (ObjectFile<E> *file : ctx.objs) {
      if (!file->lto_module) {
        for (i64 i = 0; i < file->mach_syms.size(); i++) {
          MachSym &msym = file->mach_syms[i];
          Symbol<E> &sym = *file->syms[i];
          if (msym.is_undef() && !sym.file->is_dylib &&
              ((ObjectFile<E> *)sym.file)->lto_module)
            ctx.lto.codegen_add_must_preserve_symbol(cg, sym.name.data());
        }
      }
    }

    for (ObjectFile<E> *file : ctx.objs)
      if (file->lto_module)
        for (Symbol<E> *sym : file->syms)
          if (sym->file == file && sym->scope != SCOPE_LOCAL)
            ctx.lto.codegen_add_must_preserve_symbol(cg, sym->name.data());
  }

  if (ctx.arg.entry->file)
    ctx.lto.codegen_add_must_preserve_symbol(cg, ctx.arg.entry->name.data());

  // Run the compiler backend to do LTO.
  size_t size;
  u8 *data = (u8 *)ctx.lto.codegen_compile(cg, &size);
  if (!data)
    Fatal(ctx) << "lto_codegen_compile failed: " << ctx.lto.get_error_message();

  if (!ctx.arg.object_path_lto.empty()) {
    FILE *out = fopen(ctx.arg.object_path_lto.c_str(), "w");
    if (!out)
      Fatal(ctx) << "-object_path_lto: cannot open " << ctx.arg.object_path_lto
                 << ": " << errno_string();
    fwrite(data, size, 1, out);
    fclose(out);
  }

  // Remove bitcode object files from ctx.objs.
  for (ObjectFile<E> *file : ctx.objs) {
    if (file->lto_module) {
      file->clear_symbols();
      file->is_alive = false;
    }
  }

  // Add a result of LTO as a new object file.
  MappedFile<Context<E>> *mf = new MappedFile<Context<E>>;
  mf->name = "<LTO>";
  mf->data = data;
  mf->size = size;
  ctx.mf_pool.emplace_back(mf);

  ObjectFile<E> *obj = ObjectFile<E>::create(ctx, mf, "");
  obj->parse(ctx);
  obj->is_alive = true;
  obj->priority = 100; // regular file starts with priority 10,000
  obj->resolve_symbols(ctx);
  ctx.objs.push_back(obj);
}

#ifdef MOLD_ARM64
LTOPlugin::~LTOPlugin() {
  if (dlopen_handle)
    dlclose(dlopen_handle);
}
#endif

using E = MOLD_TARGET;

template void load_lto_plugin(Context<E> &);
template void do_lto(Context<E> &);

} // namespace mold::macho
