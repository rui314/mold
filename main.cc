#include "chibild.h"

#include <iostream>

using llvm::file_magic;
using llvm::object::Archive;
using llvm::opt::InputArgList;

Config config;
SymbolTable symbol_table;

//
// Command-line option processing
//

enum {
  OPT_INVALID = 0,
#define OPTION(_1, _2, ID, _4, _5, _6, _7, _8, _9, _10, _11, _12) OPT_##ID,
#include "options.inc"
#undef OPTION
};

// Create prefix string literals used in Options.td
#define PREFIX(NAME, VALUE) const char *const NAME[] = VALUE;
#include "options.inc"
#undef PREFIX

// Create table mapping all options defined in Options.td
static const llvm::opt::OptTable::Info opt_info[] = {
#define OPTION(X1, X2, ID, KIND, GROUP, ALIAS, X7, X8, X9, X10, X11, X12)      \
  {X1, X2, X10,         X11,         OPT_##ID, llvm::opt::Option::KIND##Class, \
   X9, X8, OPT_##GROUP, OPT_##ALIAS, X7,       X12},
#include "options.inc"
#undef OPTION
};

class MyOptTable : llvm::opt::OptTable {
public:
  MyOptTable() : OptTable(opt_info) {}
  InputArgList parse(int argc, char **argv);
};

InputArgList MyOptTable::parse(int argc, char **argv) {
  unsigned missingIndex;
  unsigned missingCount;
  SmallVector<const char *, 256> vec(argv, argv + argc);

  InputArgList args = this->ParseArgs(vec, missingIndex, missingCount);
  if (missingCount)
    error(Twine(args.getArgString(missingIndex)) + ": missing argument");

  for (auto *arg : args.filtered(OPT_UNKNOWN))
    error("unknown argument '" + arg->getAsString(args) + "'");
  return args;
}

//
// Main
//

static std::vector<MemoryBufferRef> getArchiveMembers(MemoryBufferRef mb) {
  std::unique_ptr<Archive> file =
    CHECK(Archive::create(mb), mb.getBufferIdentifier() + ": failed to parse archive");

  std::vector<MemoryBufferRef> vec;

  Error err = Error::success();

  for (const Archive::Child &c : file->children(err)) {
    MemoryBufferRef mbref =
        CHECK(c.getMemoryBufferRef(),
              mb.getBufferIdentifier() +
                  ": could not get the buffer for a child of the archive");
    vec.push_back(mbref);
  }

  if (err)
    error(mb.getBufferIdentifier() + ": Archive::children failed: " +
          toString(std::move(err)));

  file.release(); // leak
  return vec;
}

static std::vector<ObjectFile *> read_file(StringRef path) {
  MemoryBufferRef mb = readFile(path);
  std::vector<ObjectFile *> vec;

  switch (identify_magic(mb.getBuffer())) {
  case file_magic::archive:
    for (MemoryBufferRef member : getArchiveMembers(mb))
      vec.push_back(new ObjectFile(member, path));
    break;
  case file_magic::elf_relocatable:
    vec.push_back(new ObjectFile(mb, ""));
    break;
  default:
    error(path + ": unknown file type");
  }
  return vec;
}

llvm::TimerGroup timers("all", "all");

static void mark_live(ObjectFile *file) {
  if (file->is_alive)
    return;

  file->is_alive = true;
  for (ObjectFile *other : file->liveness_edges)
    mark_live(other);
}

int main(int argc, char **argv) {
  MyOptTable opt_table;
  InputArgList args = opt_table.parse(argc - 1, argv + 1);

  if (auto *arg = args.getLastArg(OPT_o))
    config.output = arg->getValue();
  else
    error("-o option is missing");

  std::vector<ObjectFile *> files;

  // Open input files
  for (auto *arg : args)
    if (arg->getOption().getID() == OPT_INPUT)
      for (ObjectFile *file : read_file(arg->getValue()))
        files.push_back(file);

  // Resolve symbols
  tbb::parallel_for_each(
    files.begin(), files.end(),
    [&](ObjectFile *file) { file->parse(); });

  tbb::parallel_for_each(
    files.begin(), files.end(),
    [](ObjectFile *file) { file->register_defined_symbols(); });

  tbb::parallel_for_each(
    files.begin(), files.end(),
    [](ObjectFile *file) { file->register_undefined_symbols(); });

  // Liveness propagation
  for (ObjectFile *file : files)
    if (file->archive_name == "")
      mark_live(file);

  // Remove archive members that weren't used by any live
  // object files.
  files.erase(std::remove_if(files.begin(), files.end(),
                             [](ObjectFile *file) { return !file->is_alive; }),
              files.end());
  
  // Create output sections
  std::vector<OutputSection *> output_sections;
  for (ObjectFile *file : files) {
    for (InputSection *isec : file->sections) {
      auto *osec = new OutputSection(isec->name);
      osec->sections.push_back(isec);
      output_sections.push_back(osec);
    }
  }

  int64_t filesize = 0;
  for (OutputSection *sec : output_sections) {
    sec->set_file_offset(filesize);
    filesize += sec->get_on_file_size();
  }

  llvm::errs() << "output_sections=" << output_sections.size() << "\n"
               << "       filesize=" << filesize << "\n";

  llvm::errs() << "    files=" << files.size() << "\n"
               << "  defined=" << num_defined << "\n"
               << "undefined=" << num_undefined << "\n";

  write();
  return 0;
}
