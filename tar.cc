#include "mold.h"

// A tar file consists of one or more Ustar header followed by data.
// Each Ustar header represents a single file in an archive.
//
// tar is an old file format, and its `name` field is only 100 bytes long.
// If `name` is longer than 100 bytes, we can emit a PAX header before a
// Ustar header to store a long filename.
//
// For simplicity, we always emit a PAX header even for a short filename.
struct UstarHeader {
  UstarHeader() {
    memset(this, 0, sizeof(*this));
    memset(checksum, ' ', sizeof(checksum));
    memcpy(magic, "ustar", 5);
    memcpy(version, "00", 2);
  }

  void compute_checksum() {
    int sum = 0;
    for (i64 i = 0; i < sizeof(*this); i++)
      sum += ((u8 *)this)[i];
    sprintf(checksum, "%06o", sum);
  }

  char name[100];
  char mode[8];
  char uid[8];
  char gid[8];
  char size[12];
  char mtime[12];
  char checksum[8];
  char typeflag[1];
  char linkname[100];
  char magic[6];
  char version[2];
  char uname[32];
  char gname[32];
  char devmajor[8];
  char devminor[8];
  char prefix[155];
  char pad[12];
};

static constexpr int BLOCK_SIZE = 512;

template <typename E>
std::unique_ptr<TarFile>
TarFile::open(Context<E> &ctx, std::string path, std::string basedir) {
  std::ofstream out;
  out.open(path.c_str(),
           std::ofstream::out | std::ofstream::binary | std::ofstream::trunc);
  if (out.fail())
    Fatal(ctx) << "cannot open " << path << ": " << strerror(errno);
  return std::unique_ptr<TarFile>(new TarFile(std::move(out), basedir));
}

static void write_pax_hdr(std::ostream &out, const std::string &path) {
  // Construct a string which contains something like
  // "16 path=foo/bar\n" where 16 is the size of the string
  // including the size string itself.
  i64 len = std::string(" path=\n").size() + path.size();
  i64 total = std::to_string(len).size() + len;
  total = std::to_string(total).size() + len;
  std::string attr = std::to_string(total) + " path=" + path + "\n";

  UstarHeader hdr;
  sprintf(hdr.size, "%011zo", attr.size());
  hdr.typeflag[0] = 'x';
  hdr.compute_checksum();

  out << std::string_view((char *)&hdr, sizeof(hdr))
      << attr;
  out.seekp(align_to(out.tellp(), BLOCK_SIZE));
}

static void write_ustar_hdr(std::ostream &out, i64 filesize) {
  UstarHeader hdr;
  memcpy(hdr.mode, "0000664", 8);
  sprintf(hdr.size, "%011zo", filesize);
  hdr.compute_checksum();
  out << std::string_view((char *)&hdr, sizeof(hdr));
}

void TarFile::append(std::string path, std::string_view data) {
  write_pax_hdr(out, path_clean(basedir + "/" + path));
  write_ustar_hdr(out, data.size());
  out << data;

  // A tar file must end with two null blocks.
  i64 pos = out.tellp();
  out.seekp(align_to(pos, BLOCK_SIZE) + BLOCK_SIZE * 2 - 1);
  out << '\0';
  out.seekp(align_to(pos, BLOCK_SIZE));
}

#define INSTANTIATE(E)                                          \
  template std::unique_ptr<TarFile>                             \
  TarFile::open(Context<E> &, std::string, std::string)

INSTANTIATE(X86_64);
INSTANTIATE(I386);
