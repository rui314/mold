#include "mold.h"

#include <regex>

static constexpr int BLOCK_SIZE = 512;

// A tar file consists of one or more Ustar header followed by data.
// Each Ustar header represents a single file in an archive.
//
// tar is an old file format, and its `name` field is only 100 bytes long.
// If a filename is longer than that, you can split it at any '/'
// and store the first half to `prefix` and the second half to `name`.
//
// If a filename still doesn't fit, you need to store it into an
// extended header, which is so-called "PAX header", and put it before
// a Ustar header.
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

std::tuple<bool, std::string, std::string> split_path(const std::string &path) {
  if (path.size() < sizeof(UstarHeader::name))
    return {false, "", path};

  static std::regex re(R"(^(.{0,137})/(.{0,100})$)", std::regex::ECMAScript);
  std::smatch m;
  if (std::regex_match(path, m, re))
    return {false, m[1], m[2]};

  return {true, "", ""};
}

static void write_pax_hdr(std::ostream &out, const std::string &path) {
  i64 len = std::string(" path=").size() + path.size();
  i64 total = std::to_string(len).size() + len;
  total = std::to_string(total).size() + len;
  std::string attr = std::to_string(total) + " path=" + path;

  UstarHeader hdr;
  sprintf(hdr.size, "%011zo", attr.size());
  hdr.typeflag[0] = 'x';
  hdr.compute_checksum();

  out << std::string_view((char *)&hdr, sizeof(hdr))
      << attr;
  out.seekp(align_to(out.tellp(), BLOCK_SIZE));
}

static void write_ustar_hdr(std::ostream &out, std::string_view prefix,
                            std::string_view name, i64 size) {
  UstarHeader hdr;
  memcpy(hdr.name, name.data(), name.size());
  memcpy(hdr.mode, "0000664", 8);
  sprintf(hdr.size, "%011zo", size);
  memcpy(hdr.prefix, prefix.data(), prefix.size());
  hdr.compute_checksum();
  out << std::string_view((char *)&hdr, sizeof(hdr));
}

template <typename E>
TarFile<E>::TarFile(Context<E> &ctx, std::string path, std::string basedir)
  : basedir(basedir) {
  out.open(path.c_str(),
           std::ofstream::out | std::ofstream::binary | std::ofstream::trunc);
  if (out.fail())
    Fatal(ctx) << "cannot open " << path << ": " << strerror(errno);
}

template <typename E>
void TarFile<E>::append(std::string_view path, std::string_view data) {
  std::string fullpath = basedir + "/" + std::string(path);

  bool needs_pax_hdr;
  std::string prefix;
  std::string name;
  std::tie(needs_pax_hdr, prefix, name) = split_path(fullpath);

  if (needs_pax_hdr) {
    write_pax_hdr(out, fullpath);
    write_ustar_hdr(out, "", "", data.size());
  } else {
    write_ustar_hdr(out, prefix, name, data.size());
  }

  out << data;

  // A tar file must end with two null blocks.
  i64 pos = out.tellp();
  out.seekp(align_to(pos + BLOCK_SIZE * 2, BLOCK_SIZE) - 1);
  out << '\0';
  out.seekp(align_to(pos, BLOCK_SIZE));
}

template class TarFile<X86_64>;
template class TarFile<I386>;
