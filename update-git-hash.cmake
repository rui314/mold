# Get a git hash value. We do not want to use git command here
# because we don't want to make git a build-time dependency.
if(EXISTS "${SOURCE_DIR}/.git/HEAD")
  file(READ "${SOURCE_DIR}/.git/HEAD" HASH)
  string(STRIP "${HASH}" HASH)

  if(HASH MATCHES "^ref: (.*)")
    set(HEAD "${CMAKE_MATCH_1}")
    if(EXISTS "${SOURCE_DIR}/.git/${HEAD}")
      file(READ "${SOURCE_DIR}/.git/${HEAD}" HASH)
    else()
      file(STRINGS "${SOURCE_DIR}/.git/packed-refs" PACKED_REFS
        REGEX "${HEAD}$")
      list(GET PACKED_REFS 0 HEAD_REF)
      string(REGEX REPLACE "^(.*) ${HEAD}$" "\\1" HASH "${HEAD_REF}")
    endif()
    string(STRIP "${HASH}" HASH)
  endif()
endif()

# Create new file contents and update a given file if necessary.
set(NEW_CONTENTS "#include <string>
namespace mold {
std::string mold_git_hash = \"${HASH}\";
}
")

if(EXISTS "${OUTPUT_FILE}")
  file(READ "${OUTPUT_FILE}" OLD_CONTENTS)
  if(NOT "${NEW_CONTENTS}" STREQUAL "${OLD_CONTENTS}")
    file(WRITE "${OUTPUT_FILE}" "${NEW_CONTENTS}")
  endif()
else()
  file(WRITE "${OUTPUT_FILE}" "${NEW_CONTENTS}")
endif()
