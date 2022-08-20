function(get_git_hash DIR OUT_VAR)
  if(EXISTS "${DIR}/.git/HEAD")
    file(READ "${DIR}/.git/HEAD" HEAD)
    string(STRIP "${HEAD}" HEAD)

    if(HEAD MATCHES "^ref: (.*)")
      file(READ "${DIR}/.git/${CMAKE_MATCH_1}" HASH)
      string(STRIP "${HASH}" HASH)
      set(${OUT_VAR} ${HASH} PARENT_SCOPE)
    else()
      set(${OUT_VAR} ${HEAD} PARENT_SCOPE)
    endif()
  endif()
endfunction()

get_git_hash("${GIT_DIR}" NEW_HASH)

set(NEW_FILE "#include <string>
namespace mold {
std::string mold_git_hash = \"${NEW_HASH}\";
}
")

if(EXISTS "${OUTPUT_FILE}")
  file(READ "${OUTPUT_FILE}" OLD_FILE)
endif()

if(NOT "${NEW_FILE}" STREQUAL "${OLD_FILE}")
  file(WRITE "${OUTPUT_FILE}" "${NEW_FILE}")
endif()
