#include "mold.h"

namespace mold {

// A tar file consists of one or more Ustar header followed by data.
// Each Ustar header represents a single file in an archive.
//
// tar is an old file format, and its `name` field is only 100 bytes long.
// If `name` is longer than 100 bytes, we can emit a PAX header before a
// Ustar header to store a long filename.
//
// For simplicity, we always emit a PAX header even for a short filename.
struct UstarHeader {
  void flush() {
    memset(checksum, ' ', sizeof(checksum));
    memcpy(magic, "ustar", 5);
    memcpy(version, "00", 2);

    // Compute checksum
    int sum = 0;
    for (i64 i = 0; i < sizeof(*this); i++)
      sum += ((u8 *)this)[i];
    assert(sum < 1000000);
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

std::string TarFile::encode_path(std::string path) {
  path = path_clean(basedir + "/" + path);

  // Construct a string which contains something like
  // "16 path=foo/bar\n" where 16 is the size of the string
  // including the size string itself.
  i64 len = std::string(" path=\n").size() + path.size();
  i64 total = std::to_string(len).size() + len;
  total = std::to_string(total).size() + len;
  return std::to_string(total) + " path=" + path + "\n";
}

void TarFile::append(std::string path, std::string_view data) {
  contents.push_back({path, data});

  size_ += BLOCK_SIZE * 2;
  size_ += align_to(encode_path(path).size(), BLOCK_SIZE);
  size_ += align_to(data.size(), BLOCK_SIZE);
}

void TarFile::write_to(u8 *buf) {
  u8 *start = buf;
  memset(buf, 0, size_);

  for (i64 i = 0; i < contents.size(); i++) {
    assert(buf - start <= size_);

    const std::string &path = contents[i].first;
    std::string_view data = contents[i].second;

    // Write PAX header
    static_assert(sizeof(UstarHeader) == BLOCK_SIZE);
    UstarHeader &pax = *(UstarHeader *)buf;
    buf += BLOCK_SIZE;

    std::string attr = encode_path(path);
    sprintf(pax.size, "%011zo", attr.size());
    pax.typeflag[0] = 'x';
    pax.flush();

    // Write pathname
    memcpy(buf, attr.data(), attr.size());
    buf += align_to(attr.size(), BLOCK_SIZE);

    // Write Ustar header
    UstarHeader &ustar = *(UstarHeader *)buf;
    buf += BLOCK_SIZE;

    memcpy(ustar.mode, "0000664", 8);
    sprintf(ustar.size, "%011zo", data.size());
    ustar.flush();

    // Write file contents
    memcpy(buf, data.data(), data.size());
    buf += align_to(data.size(), BLOCK_SIZE);
  }
}

} // namespace mold
