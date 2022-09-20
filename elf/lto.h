#pragma once

#include "../inttypes.h"

namespace mold {

enum PluginStatus {
  LDPS_OK,
  LDPS_NO_SYMS,
  LDPS_BAD_HANDLE,
  LDPS_ERR,
};

enum PluginTag {
  LDPT_NULL,
  LDPT_API_VERSION,
  LDPT_GOLD_VERSION,
  LDPT_LINKER_OUTPUT,
  LDPT_OPTION,
  LDPT_REGISTER_CLAIM_FILE_HOOK,
  LDPT_REGISTER_ALL_SYMBOLS_READ_HOOK,
  LDPT_REGISTER_CLEANUP_HOOK,
  LDPT_ADD_SYMBOLS,
  LDPT_GET_SYMBOLS,
  LDPT_ADD_INPUT_FILE,
  LDPT_MESSAGE,
  LDPT_GET_INPUT_FILE,
  LDPT_RELEASE_INPUT_FILE,
  LDPT_ADD_INPUT_LIBRARY,
  LDPT_OUTPUT_NAME,
  LDPT_SET_EXTRA_LIBRARY_PATH,
  LDPT_GNU_LD_VERSION,
  LDPT_GET_VIEW,
  LDPT_GET_INPUT_SECTION_COUNT,
  LDPT_GET_INPUT_SECTION_TYPE,
  LDPT_GET_INPUT_SECTION_NAME,
  LDPT_GET_INPUT_SECTION_CONTENTS,
  LDPT_UPDATE_SECTION_ORDER,
  LDPT_ALLOW_SECTION_ORDERING,
  LDPT_GET_SYMBOLS_V2,
  LDPT_ALLOW_UNIQUE_SEGMENT_FOR_SECTIONS,
  LDPT_UNIQUE_SEGMENT_FOR_SECTIONS,
  LDPT_GET_SYMBOLS_V3,
  LDPT_GET_INPUT_SECTION_ALIGNMENT,
  LDPT_GET_INPUT_SECTION_SIZE,
  LDPT_REGISTER_NEW_INPUT_HOOK,
  LDPT_GET_WRAP_SYMBOLS,
  LDPT_ADD_SYMBOLS_V2,
  LDPT_GET_API_VERSION,
};

enum PluginApiVersion {
  LD_PLUGIN_API_VERSION = 1,
};

struct PluginTagValue {
  PluginTagValue(PluginTag tag, int val) : tag(tag), val(val) {}

  template <typename T>
  PluginTagValue(PluginTag tag, T *ptr) : tag(tag), ptr((void *)ptr) {}

  PluginTag tag;
  union {
    int val;
    void *ptr;
  };
};

enum PluginOutputFileType {
  LDPO_REL,
  LDPO_EXEC,
  LDPO_DYN,
  LDPO_PIE,
};

struct PluginInputFile {
  const char *name;
  int32_t fd;
  uint64_t offset;
  uint64_t filesize;
  void *handle;
};

struct PluginSection {
  const void *handle;
  uint32_t shndx;
};

struct PluginSymbol {
  char *name;
  char *version;
#ifdef __LITTLE_ENDIAN__
  char def;
  char symbol_type;
  char section_kind;
  char padding;
#else
  char padding;
  char section_kind;
  char symbol_type;
  char def;
#endif
  int32_t visibility;
  uint64_t size;
  char *comdat_key;
  int32_t resolution;
};

enum PluginSymbolKind {
  LDPK_DEF,
  LDPK_WEAKDEF,
  LDPK_UNDEF,
  LDPK_WEAKUNDEF,
  LDPK_COMMON,
};

enum PluginSymbolVisibility {
  LDPV_DEFAULT,
  LDPV_PROTECTED,
  LDPV_INTERNAL,
  LDPV_HIDDEN,
};

enum PluginSymbolType {
  LDST_UNKNOWN,
  LDST_FUNCTION,
  LDST_VARIABLE,
};

enum PluginSymbolSectionKind {
  LDSSK_DEFAULT,
  LDSSK_BSS,
};

enum PluginSymbolResolution {
  LDPR_UNKNOWN,
  LDPR_UNDEF,
  LDPR_PREVAILING_DEF,
  LDPR_PREVAILING_DEF_IRONLY,
  LDPR_PREEMPTED_REG,
  LDPR_PREEMPTED_IR,
  LDPR_RESOLVED_IR,
  LDPR_RESOLVED_EXEC,
  LDPR_RESOLVED_DYN,
  LDPR_PREVAILING_DEF_IRONLY_EXP,
};

enum PluginLevel {
  LDPL_INFO,
  LDPL_WARNING,
  LDPL_ERROR,
  LDPL_FATAL,
};

enum PluginLinkerAPIVersion {
  LAPI_V0 = 0,
  LAPI_V1,
};

typedef PluginStatus OnloadFn(PluginTagValue *tv);
typedef PluginStatus ClaimFileHandler(const PluginInputFile *, int *);
typedef PluginStatus AllSymbolsReadHandler();
typedef PluginStatus CleanupHandler();
typedef PluginStatus NewInputHandler(const PluginInputFile *);

} // namespace mold
