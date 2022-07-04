#pragma once

#include <cstddef>
#include <cstdint>
#include <sys/types.h>

namespace mold::macho {

enum LTOSymbolAttributes {
  LTO_SYMBOL_ALIGNMENT_MASK = 0x0000001F,
  LTO_SYMBOL_PERMISSIONS_MASK = 0x000000E0,
  LTO_SYMBOL_PERMISSIONS_CODE = 0x000000A0,
  LTO_SYMBOL_PERMISSIONS_DATA = 0x000000C0,
  LTO_SYMBOL_PERMISSIONS_RODATA = 0x00000080,
  LTO_SYMBOL_DEFINITION_MASK = 0x00000700,
  LTO_SYMBOL_DEFINITION_REGULAR = 0x00000100,
  LTO_SYMBOL_DEFINITION_TENTATIVE = 0x00000200,
  LTO_SYMBOL_DEFINITION_WEAK = 0x00000300,
  LTO_SYMBOL_DEFINITION_UNDEFINED = 0x00000400,
  LTO_SYMBOL_DEFINITION_WEAKUNDEF = 0x00000500,
  LTO_SYMBOL_SCOPE_MASK = 0x00003800,
  LTO_SYMBOL_SCOPE_INTERNAL = 0x00000800,
  LTO_SYMBOL_SCOPE_HIDDEN = 0x00001000,
  LTO_SYMBOL_SCOPE_DEFAULT = 0x00001800,
  LTO_SYMBOL_SCOPE_PROTECTED = 0x00002000,
  LTO_SYMBOL_SCOPE_DEFAULT_CAN_BE_HIDDEN = 0x00002800,
  LTO_SYMBOL_COMDAT = 0x00004000,
  LTO_SYMBOL_ALIAS = 0x00008000,
};

enum LTODebugModel {
  LTO_DEBUG_MODEL_NONE = 0,
  LTO_DEBUG_MODEL_DWARF = 1,
};

enum LTOCodegenModel {
  LTO_CODEGEN_PIC_MODEL_STATIC = 0,
  LTO_CODEGEN_PIC_MODEL_DYNAMIC = 1,
  LTO_CODEGEN_PIC_MODEL_DYNAMIC_NO_PIC = 2,
  LTO_CODEGEN_PIC_MODEL_DEFAULT = 3,
};

enum LTOCodegenDiagnosticSeverity {
  LTO_DS_ERROR = 0,
  LTO_DS_WARNING = 1,
  LTO_DS_REMARK = 3,
  LTO_DS_NOTE = 2,
};

struct LTOObjectBuffer {
  const char *buffer;
  size_t size;
};

using LTOModule = void;
using LTOCodeGen = void;

typedef void LTODiagnosticHandler(LTOCodegenDiagnosticSeverity severity,
                                  const char *diag, void *ctxt);

struct LTOPlugin {
  void *dlopen_handle = nullptr;
  ~LTOPlugin();

  char (*get_version)();

  char (*get_error_message)();

  bool (*module_is_object_file)(const char *path);

  bool (*module_is_object_file_for_target)(const char *path,
                                           const char *target_triple_prefix);

  bool (*module_has_objc_category)(const void *mem, size_t length);

  bool (*module_is_object_file_in_memory)(const void *mem, size_t length);

  bool (*module_is_object_file_in_memory_for_target)(
      const void *mem, size_t length, const char *target_triple_prefix);

  LTOModule *(*module_create)(const char *path);

  LTOModule *(*module_create_from_memory)(const void *mem, size_t length);

  LTOModule *(*module_create_from_memory_with_path)(const void *mem,
                                                   size_t length,
                                                   const char *path);

  LTOModule *(*module_create_in_local_context)(const void *mem, size_t length,
                                              const char *path);

  LTOModule *(*module_create_in_codegen_context)(const void *mem, size_t length,
                                                const char *path,
                                                LTOCodeGen *cg);

  LTOModule *(*module_create_from_fd)(int fd, const char *path,
                                     size_t file_size);

  LTOModule *(*module_create_from_fd_at_offset)(int fd, const char *path,
                                               size_t file_size,
                                               size_t map_size, off_t offset);

  void (*module_dispose)(LTOModule *mod);

  const char *(*module_get_target_triple)(LTOModule *mod);

  void (*module_set_target_triple)(LTOModule *mod, const char *triple);

  unsigned (*module_get_num_symbols)(LTOModule *mod);

  const char *(*module_get_symbol_name)(LTOModule *mod, unsigned index);

  uint32_t (*module_get_symbol_attribute)(LTOModule *mod, unsigned index);

  const char *(*module_get_linkeropts)(LTOModule *mod);

  bool (*module_get_macho_cputype)(LTOModule *mod, unsigned *out_cputype,
                                   unsigned *out_cpusubtype);

  bool (*module_has_ctor_dtor)(LTOModule *mod);

  void (*codegen_set_diagnostic_handler)(LTOCodeGen *cg, LTODiagnosticHandler,
                                         void *);

  LTOCodeGen *(*codegen_create)();

  LTOCodeGen *(*codegen_create_in_local_context)();

  void (*codegen_dispose)(LTOCodeGen *cg);

  bool (*codegen_add_module)(LTOCodeGen *cg, LTOModule *mod);

  void (*codegen_set_module)(LTOCodeGen *cg, LTOModule *mod);

  bool (*codegen_set_debug_model)(LTOCodeGen *cg, LTODebugModel);

  bool (*codegen_set_pic_model)(LTOCodeGen *cg, LTOCodegenModel);

  void (*codegen_set_cpu)(LTOCodeGen *cg, const char *cpu);

  void (*codegen_set_assembler_path)(LTOCodeGen *cg, const char *path);

  void (*codegen_set_assembler_args)(LTOCodeGen *cg, const char **args,
                                     int nargs);

  void (*codegen_add_must_preserve_symbol)(LTOCodeGen *cg, const char *symbol);

  bool (*codegen_write_merged_modules)(LTOCodeGen *cg, const char *path);

  const void *(*codegen_compile)(LTOCodeGen *cg, size_t *length);

  bool (*codegen_compile_to_file)(LTOCodeGen *cg, const char **name);

  bool (*codegen_optimize)(LTOCodeGen *cg);

  const void *(*codegen_compile_optimized)(LTOCodeGen *cg, size_t *length);

  unsigned (*api_version)();

  void (*set_debug_options)(const char *const *options, int number);

  void (*codegen_debug_options)(LTOCodeGen *cg, const char *);

  void (*codegen_debug_options_array)(LTOCodeGen *cg, const char *const *,
                                      int number);

  void (*initialize_disassembler)();

  void (*codegen_set_should_internalize)(LTOCodeGen *cg,
                                         bool should_internalize);

  void (*codegen_set_should_embed_uselists)(LTOCodeGen *cg,
                                            bool should_embed_use_lists);
};

} // namespace mold::macho
