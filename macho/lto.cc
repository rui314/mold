#include "lto.h"
#include "mold.h"

#include <dlfcn.h>

namespace mold::macho {

template <typename E> static void load_plugin(Context<E> &ctx) {
  void *handle = dlopen(ctx.arg.lto_library.c_str(), RTLD_NOW | RTLD_GLOBAL);
  if (!handle)
    Fatal(ctx) << "could not open plugin file: " << dlerror();

  Plugin p = {};
  p.dlopen_handle = handle;

  auto set = [&](auto &lhs, const char *sym) {
    lhs = (decltype(lhs))dlsym(handle, sym);
  };

  set(p.get_version, "lto_get_version");
  set(p.get_error_message, "lto_get_error_message");
  set(p.module_is_object_file, "lto_module_is_object_file");
  set(p.module_is_object_file_for_target,
      "lto_module_is_object_file_for_target");
  set(p.module_has_objc_category, "lto_module_has_objc_category");
  set(p.module_is_object_file_in_memory, "lto_module_is_object_file_in_memory");
  set(p.module_is_object_file_in_memory_for_target,
      "lto_module_is_object_file_in_memory_for_target");
  set(p.module_create, "lto_module_create");
  set(p.module_create_from_memory, "lto_module_create_from_memory");
  set(p.module_create_from_memory_with_path,
      "lto_module_create_from_memory_with_path");
  set(p.module_create_in_local_context, "lto_module_create_in_local_context");
  set(p.module_create_in_codegen_context,
      "lto_module_create_in_codegen_context");
  set(p.module_create_from_fd, "lto_module_create_from_fd");
  set(p.module_create_from_fd_at_offset, "lto_module_create_from_fd_at_offset");
  set(p.module_dispose, "lto_module_dispose");
  set(p.module_get_target_triple, "lto_module_get_target_triple");
  set(p.module_set_target_triple, "lto_module_set_target_triple");
  set(p.module_get_num_symbols, "lto_module_get_num_symbols");
  set(p.module_get_symbol_name, "lto_module_get_symbol_name");
  set(p.module_get_symbol_attribute, "lto_module_get_symbol_attribute");
  set(p.module_get_linkeropts, "lto_module_get_linkeropts");
  set(p.module_get_macho_cputype, "lto_module_get_macho_cputype");
  set(p.module_has_ctor_dtor, "lto_module_has_ctor_dtor");
  set(p.codegen_set_diagnostic_handler, "lto_codegen_set_diagnostic_handler");
  set(p.codegen_create, "lto_codegen_create");
  set(p.codegen_create_in_local_context, "lto_codegen_create_in_local_context");
  set(p.codegen_dispose, "lto_codegen_dispose");
  set(p.codegen_add_module, "lto_codegen_add_module");
  set(p.codegen_set_module, "lto_codegen_set_module");
  set(p.codegen_set_debug_model, "lto_codegen_set_debug_model");
  set(p.codegen_set_pic_model, "lto_codegen_set_pic_model");
  set(p.codegen_set_cpu, "lto_codegen_set_cpu");
  set(p.codegen_set_assembler_path, "lto_codegen_set_assembler_path");
  set(p.codegen_set_assembler_args, "lto_codegen_set_assembler_args");
  set(p.codegen_add_must_preserve_symbol,
      "lto_codegen_add_must_preserve_symbol");
  set(p.codegen_write_merged_modules, "lto_codegen_write_merged_modules");
  set(p.codegen_compile, "lto_codegen_compile");
  set(p.codegen_compile_to_file, "lto_codegen_compile_to_file");
  set(p.codegen_optimize, "lto_codegen_optimize");
  set(p.codegen_compile_optimized, "lto_codegen_compile_optimized");
  set(p.api_version, "lto_api_version");
  set(p.set_debug_options, "lto_set_debug_options");
  set(p.codegen_debug_options, "lto_codegen_debug_options");
  set(p.codegen_debug_options_array, "lto_codegen_debug_options_array");
  set(p.initialize_disassembler, "lto_initialize_disassembler");
  set(p.codegen_set_should_internalize, "lto_codegen_set_should_internalize");
  set(p.codegen_set_should_embed_uselists,
      "lto_codegen_set_should_embed_uselists");
}

} // namespace mold::macho
