#include "mold.h"
#include "../archive-file.h"
#include "../cmdline.h"
#include "../output-file.h"

#include <cstdlib>
#include <fcntl.h>
#include <iomanip>
#include <iostream>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>

namespace mold::macho {

static void create_internal_file(Context &ctx) {
  ObjectFile *obj = new ObjectFile;
  ctx.obj_pool.push_back(std::unique_ptr<ObjectFile>(obj));
  ctx.objs.push_back(obj);

  auto add = [&](std::string_view name, u64 value = 0) {
    Symbol *sym = intern(ctx, name);
    sym->file = obj;
    sym->value = value;
    obj->syms.push_back(sym);
  };

  add("__dyld_private");
  add("__mh_execute_header", PAGE_ZERO_SIZE);

  obj->syms.push_back(intern(ctx, "dyld_stub_binder"));
}

static void add_section(Context &ctx, OutputSection &osec,
                        std::string_view segname, std::string_view sectname) {
  for (ObjectFile *obj : ctx.objs) {
    for (std::unique_ptr<InputSection> &sec : obj->sections) {
      if (sec->hdr.segname == segname && sec->hdr.sectname == sectname) {
        for (Subsection &subsec : sec->subsections)
          osec.members.push_back(&subsec);
        sec->osec = &osec;
      }
    }
  }
}

static void create_synthetic_chunks(Context &ctx) {
  ctx.segments.push_back(&ctx.text_seg);
  ctx.segments.push_back(&ctx.data_const_seg);
  ctx.segments.push_back(&ctx.data_seg);
  ctx.segments.push_back(&ctx.linkedit_seg);

  ctx.text_seg.chunks.push_back(&ctx.mach_hdr);
  ctx.text_seg.chunks.push_back(&ctx.load_cmd);
  ctx.text_seg.chunks.push_back(&ctx.padding);

  ctx.padding.hdr.size = 14808;

  ctx.text.hdr.attr = S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS;
  ctx.text.hdr.p2align = 4;
  add_section(ctx, ctx.text, "__TEXT", "__text");
  ctx.text_seg.chunks.push_back(&ctx.text);

  ctx.text_seg.chunks.push_back(&ctx.stubs);
  ctx.text_seg.chunks.push_back(&ctx.stub_helper);

  OutputSection *cstring = new OutputSection("__cstring");
  cstring->hdr.type = S_CSTRING_LITERALS;
  add_section(ctx, *cstring, "__TEXT", "__cstring");
  ctx.text_seg.chunks.push_back(cstring);

  ctx.text_seg.chunks.push_back(&ctx.unwind_info);

  ctx.data_const_seg.chunks.push_back(&ctx.got);

  ctx.data_seg.chunks.push_back(&ctx.lazy_symbol_ptr);
  ctx.data_seg.chunks.push_back(&ctx.data);

  ctx.linkedit_seg.chunks.push_back(&ctx.rebase);
  ctx.linkedit_seg.chunks.push_back(&ctx.bind);
  ctx.linkedit_seg.chunks.push_back(&ctx.lazy_bind);
  ctx.linkedit_seg.chunks.push_back(&ctx.export_);
  ctx.linkedit_seg.chunks.push_back(&ctx.function_starts);
  ctx.linkedit_seg.chunks.push_back(&ctx.symtab);
  ctx.linkedit_seg.chunks.push_back(&ctx.indir_symtab);
  ctx.linkedit_seg.chunks.push_back(&ctx.strtab);
}

static void fill_symtab(Context &ctx) {
  ctx.symtab.add(ctx, "__dyld_private", N_SECT, false, 8, 0x0, 0x100008008);
  ctx.symtab.add(ctx, "__mh_execute_header", N_SECT, true, 1, 0x10, 0x100000000);
  ctx.symtab.add(ctx, "_hello", N_SECT, true, 1, 0x0, 0x100003f50);
  ctx.symtab.add(ctx, "_main", N_SECT, true, 1, 0x0, 0x100003f70);
  ctx.symtab.add(ctx, "_printf", N_UNDF, true, 0, 0x100, 0x0);
  ctx.symtab.add(ctx, "dyld_stub_binder", N_UNDF, true, 0, 0x100, 0x0);

  ctx.strtab.hdr.size = align_to(ctx.strtab.hdr.size, 8);
}

static void export_symbols(Context &ctx) {
  ctx.stubs.add(ctx, *intern(ctx, "_printf"), 1, 0, 3, 0);
}

static i64 assign_offsets(Context &ctx) {
  i64 fileoff = 0;
  i64 vmaddr = PAGE_ZERO_SIZE;

  for (OutputSegment *seg : ctx.segments) {
    seg->set_offset(ctx, fileoff, vmaddr);
    fileoff += seg->cmd.filesize;
    vmaddr += seg->cmd.vmsize;
  }
  return fileoff;
}

static void fix_synthetic_symbol_values(Context &ctx) {
  intern(ctx, "__dyld_private")->value = ctx.data.hdr.addr;
}

void read_file(Context &ctx, MappedFile<Context> *mf) {
  switch (get_file_type(mf)) {
  case FileType::MACH_OBJ:
    ctx.objs.push_back(ObjectFile::create(ctx, mf));
    return;
  default:
    Fatal(ctx) << mf->name << ": unknown file type";
  }
}

int main(int argc, char **argv) {
  Context ctx;

  if (argc > 1 && std::string_view(argv[1]) == "-dump") {
    if (argc != 3)
      Fatal(ctx) << "usage: ld64.mold -dump <executable-name>\n";
    dump_file(argv[2]);
    exit(0);
  }

  if (argc > 1 && std::string_view(argv[1]) == "-yamltest") {
    std::vector<YamlNode> nodes = parse_yaml(ctx, R"(
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, x86_64-maccatalyst, arm64-macos, arm64-maccatalyst,
                   arm64e-macos, arm64e-maccatalyst ]
uuids:
  - target:          x86_64-macos
    value:           89E331F9-9A00-3EA4-B49F-FA2B91AE9182
  - target:          x86_64-maccatalyst
    value:           89E331F9-9A00-3EA4-B49F-FA2B91AE9182
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-maccatalyst
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64e-macos
    value:           A9F7E132-0FFC-31FC-83C6-3848CA460DF3
  - target:          arm64e-maccatalyst
    value:           A9F7E132-0FFC-31FC-83C6-3848CA460DF3
install-name:    '/usr/lib/libSystem.B.dylib'
current-version: 1292.100.5
reexported-libraries:
  - targets:         [ x86_64-macos, x86_64-maccatalyst, arm64-macos, arm64-maccatalyst,
                       arm64e-macos, arm64e-maccatalyst ]
    libraries:       [ '/usr/lib/system/libcache.dylib', '/usr/lib/system/libcommonCrypto.dylib',
                       '/usr/lib/system/libcompiler_rt.dylib', '/usr/lib/system/libcopyfile.dylib',
                       '/usr/lib/system/libcorecrypto.dylib', '/usr/lib/system/libdispatch.dylib',
                       '/usr/lib/system/libdyld.dylib', '/usr/lib/system/libkeymgr.dylib',
                       '/usr/lib/system/liblaunch.dylib', '/usr/lib/system/libmacho.dylib',
                       '/usr/lib/system/libquarantine.dylib', '/usr/lib/system/libremovefile.dylib',
                       '/usr/lib/system/libsystem_asl.dylib', '/usr/lib/system/libsystem_blocks.dylib',
                       '/usr/lib/system/libsystem_c.dylib', '/usr/lib/system/libsystem_collections.dylib',
                       '/usr/lib/system/libsystem_configuration.dylib', '/usr/lib/system/libsystem_containermanager.dylib',
                       '/usr/lib/system/libsystem_coreservices.dylib', '/usr/lib/system/libsystem_darwin.dylib',
                       '/usr/lib/system/libsystem_dnssd.dylib', '/usr/lib/system/libsystem_featureflags.dylib',
                       '/usr/lib/system/libsystem_info.dylib', '/usr/lib/system/libsystem_kernel.dylib',
                       '/usr/lib/system/libsystem_m.dylib', '/usr/lib/system/libsystem_malloc.dylib',
                       '/usr/lib/system/libsystem_networkextension.dylib', '/usr/lib/system/libsystem_notify.dylib',
                       '/usr/lib/system/libsystem_platform.dylib', '/usr/lib/system/libsystem_product_info_filter.dylib',
                       '/usr/lib/system/libsystem_pthread.dylib', '/usr/lib/system/libsystem_sandbox.dylib',
                       '/usr/lib/system/libsystem_secinit.dylib', '/usr/lib/system/libsystem_symptoms.dylib',
                       '/usr/lib/system/libsystem_trace.dylib', '/usr/lib/system/libunwind.dylib',
                       '/usr/lib/system/libxpc.dylib' ]
exports:
  - targets:         [ x86_64-maccatalyst, x86_64-macos ]
    symbols:         [ 'R8289209$_close', 'R8289209$_fork', 'R8289209$_fsync', 'R8289209$_getattrlist',
                       'R8289209$_getrlimit', 'R8289209$_getxattr', 'R8289209$_open',
                       'R8289209$_pthread_attr_destroy', 'R8289209$_pthread_attr_init',
                       'R8289209$_pthread_attr_setdetachstate', 'R8289209$_pthread_create',
                       'R8289209$_pthread_mutex_lock', 'R8289209$_pthread_mutex_unlock',
                       'R8289209$_pthread_self', 'R8289209$_ptrace', 'R8289209$_read',
                       'R8289209$_setattrlist', 'R8289209$_setrlimit', 'R8289209$_sigaction',
                       'R8289209$_stat', 'R8289209$_sysctl', 'R8289209$_time', 'R8289209$_unlink',
                       'R8289209$_write' ]
  - targets:         [ x86_64-maccatalyst, x86_64-macos, arm64e-maccatalyst, arm64e-macos,
                       arm64-macos, arm64-maccatalyst ]
    symbols:         [ ___crashreporter_info__, _libSystem_atfork_child, _libSystem_atfork_parent,
                       _libSystem_atfork_prepare, _mach_init_routine ]
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, x86_64-maccatalyst, arm64-macos, arm64-maccatalyst,
                   arm64e-macos, arm64e-maccatalyst ]
uuids:
  - target:          x86_64-macos
    value:           ED7D7EB4-B248-33A9-9E6A-58F45EAB7602
  - target:          x86_64-maccatalyst
    value:           ED7D7EB4-B248-33A9-9E6A-58F45EAB7602
  - target:          arm64-macos
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64-maccatalyst
    value:           00000000-0000-0000-0000-000000000000
  - target:          arm64e-macos
    value:           758F8B92-8581-3370-9F97-1E3AB045122F
  - target:          arm64e-maccatalyst
    value:           758F8B92-8581-3370-9F97-1E3AB045122F
install-name:    '/usr/lib/system/libcache.dylib'
current-version: 83
parent-umbrella:
  - targets:         [ x86_64-macos, x86_64-maccatalyst, arm64-macos, arm64-maccatalyst,
                       arm64e-macos, arm64e-maccatalyst ]
    umbrella:        System
exports:
  - targets:         [ arm64e-macos, x86_64-macos, x86_64-maccatalyst, arm64e-maccatalyst,
                       arm64-macos, arm64-maccatalyst ]
    symbols:         [ _cache_create, _cache_destroy, _cache_get, _cache_get_and_retain,
                       _cache_get_cost_hint, _cache_get_count_hint, _cache_get_info,
                       _cache_get_info_for_key, _cache_get_info_for_keys, _cache_get_minimum_values_hint,
                       _cache_get_name, _cache_hash_byte_string, _cache_invoke, _cache_key_hash_cb_cstring,
                       _cache_key_hash_cb_integer, _cache_key_is_equal_cb_cstring,
                       _cache_key_is_equal_cb_integer, _cache_print, _cache_print_stats,
                       _cache_release_cb_free, _cache_release_value, _cache_remove,
                       _cache_remove_all, _cache_remove_with_block, _cache_set_and_retain,
                       _cache_set_cost_hint, _cache_set_count_hint, _cache_set_minimum_values_hint,
                       _cache_set_name, _cache_simulate_memory_warning_event, _cache_value_make_nonpurgeable_cb,
                       _cache_value_make_purgeable_cb ]
)");

    for (YamlNode &node : nodes) {
      SyncOut(ctx) << "---";
      dump_yaml(ctx, node);
    }
    exit(0);
  }

  ctx.cmdline_args = expand_response_files(ctx, argv);
  std::vector<std::string_view> file_args;
  parse_nonpositional_args(ctx, file_args);

  for (std::string_view arg : file_args)
    read_file(ctx, MappedFile<Context>::must_open(ctx, std::string(arg)));

  for (ObjectFile *obj : ctx.objs)
    obj->parse(ctx);

  for (ObjectFile *obj : ctx.objs)
    obj->resolve_symbols(ctx);

  create_internal_file(ctx);
  create_synthetic_chunks(ctx);
  fill_symtab(ctx);
  export_symbols(ctx);
  i64 output_size = assign_offsets(ctx);
  fix_synthetic_symbol_values(ctx);

  ctx.output_file = open_output_file(ctx, ctx.arg.output, output_size, 0777);
  ctx.buf = ctx.output_file->buf;

  for (OutputSegment *seg : ctx.segments)
    seg->copy_buf(ctx);

  ctx.output_file->close(ctx);
  return 0;
}

}
