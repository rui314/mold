#include "mold.h"

using namespace llvm;
using namespace llvm::ELF;
using llvm::object::ELF64LE;

static std::unique_ptr<FileOutputBuffer> open_output_file() {
  int file_size = 100;
  Expected<std::unique_ptr<FileOutputBuffer>> bufferOrErr =
    FileOutputBuffer::create(config.output, file_size, 0);

  if (!bufferOrErr)
    error("failed to open " + config.output + ": " +
          llvm::toString(bufferOrErr.takeError()));
  return std::move(*bufferOrErr);
}

void write() {
  std::unique_ptr<FileOutputBuffer> buffer = open_output_file();
  uint8_t *buf = buffer->getBufferStart();

  memset(buf, 0, sizeof(ELF64LE::Ehdr));
  memcpy(buf, "\177ELF", 4);

  auto *ehdr = reinterpret_cast<ELF64LE::Ehdr *>(buf);
  ehdr->e_ident[EI_CLASS] = ELFCLASS64;
  ehdr->e_ident[EI_DATA] = ELFDATA2LSB;
  ehdr->e_ident[EI_VERSION] = EV_CURRENT;
  ehdr->e_ident[EI_OSABI] = 0;
  ehdr->e_ident[EI_ABIVERSION] = 0;
  ehdr->e_machine = EM_X86_64;
  ehdr->e_version = EV_CURRENT;
  ehdr->e_flags = 0;
  ehdr->e_ehsize = sizeof(ELF64LE::Ehdr);
  ehdr->e_phnum = 0;
  ehdr->e_shentsize = sizeof(ELF64LE::Shdr);
  ehdr->e_phoff = sizeof(ELF64LE::Ehdr);
  ehdr->e_phentsize = sizeof(ELF64LE::Phdr);

  if (auto e = buffer->commit())
    error("failed to write to the output file: " + toString(std::move(e)));
}
