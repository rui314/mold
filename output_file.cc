#include "chibild.h"

using namespace llvm;
using namespace llvm::ELF;
using llvm::object::ELF64LE;

OutputFile::OutputFile(uint64_t size) {
  Expected<std::unique_ptr<FileOutputBuffer>> bufOrErr =
    FileOutputBuffer::create(config.output, size, 0);

  if (!bufOrErr)
    error("failed to open " + config.output + ": " +
          llvm::toString(bufOrErr.takeError()));
  output_buffer = std::move(*bufOrErr);
  buf = output_buffer->getBufferStart();
}

void OutputFile::commit() {
  if (auto e = output_buffer->commit())
    error("failed to write to the output file: " + toString(std::move(e)));
}
