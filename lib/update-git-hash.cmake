# Embed the current git commit hash into the mold executable. We ask git
# instead of parsing files under .git because the repository format varies
# (e.g. packed refs, reftable). When building from a source tarball, there's
# no .git, and the hash is simply omitted.
if(EXISTS "${SOURCE_DIR}/.git")
  execute_process(
    COMMAND git -C "${SOURCE_DIR}" rev-parse HEAD
    OUTPUT_VARIABLE HASH
    OUTPUT_STRIP_TRAILING_WHITESPACE
    ERROR_QUIET
    RESULT_VARIABLE EXIT_CODE)

  if(NOT EXIT_CODE EQUAL 0)
    set(HASH "")
  endif()
endif()

# Create new file contents and update a given file if necessary.
if("${HASH}" STREQUAL "")
  set(NEW_CONTENTS "")
else()
  set(NEW_CONTENTS "#define MOLD_GIT_HASH \"${HASH}\"\n")
endif()

if(EXISTS "${OUTPUT_FILE}")
  file(READ "${OUTPUT_FILE}" OLD_CONTENTS)
  if(NOT "${NEW_CONTENTS}" STREQUAL "${OLD_CONTENTS}")
    file(WRITE "${OUTPUT_FILE}" "${NEW_CONTENTS}")
  endif()
else()
  file(WRITE "${OUTPUT_FILE}" "${NEW_CONTENTS}")
endif()
