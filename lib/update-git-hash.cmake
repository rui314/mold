# Get a git hash value. We do not want to use git command here
# because we don't want to make git a build-time dependency.
if(EXISTS "${SOURCE_DIR}/.git/HEAD")
  file(READ "${SOURCE_DIR}/.git/HEAD" HASH)
  string(STRIP "${HASH}" HASH)

  if(HASH MATCHES "^ref: (.*)")
    set(HEAD "${CMAKE_MATCH_1}")
    if(HEAD STREQUAL "refs/heads/.invalid")
      # Git v2.45+ with config `feature.experimental`=true, config `extensions.refstorage`=reftable,
      # or `--ref-format=reftable`, uses the new binary reftable format, which does not expose
      # plaintext files and breaks our no-Git wishes as the HEAD hash is not directly stored.
      #
      # Provide an option to disable Git and fall back to no hash if desired. Else, best-effort
      # attempt to find ans use Git.
      option(MOLD_NO_GIT "Do not look for Git if the new reftable format is in use" OFF)
      if(MOLD_NO_GIT)
        set(HASH "")
      else()
        find_package(Git QUIET)
        if (NOT Git_FOUND)
          message(WARNING "In a git repo with a reftable format, but could not find Git. No hash will be embedded in the binary.")
          set(HASH "")
        else()
          execute_process(
            COMMAND ${GIT_EXECUTABLE} rev-parse HEAD
            OUTPUT_VARIABLE git_output
            RESULT_VARIABLE git_result
            ERROR_QUIET
            OUTPUT_STRIP_TRAILING_WHITESPACE
          )
          if(git_result EQUAL 0)
            set(HASH "${git_output}")
          else()
            message(WARNING "Git returned ${git_result} when querying the HEAD revision")
          endif()
        endif()
      endif()
    elseif(EXISTS "${SOURCE_DIR}/.git/${HEAD}")
      file(READ "${SOURCE_DIR}/.git/${HEAD}" HASH)
      string(STRIP "${HASH}" HASH)
    else()
      file(READ "${SOURCE_DIR}/.git/packed-refs" PACKED_REFS)
      string(REGEX REPLACE ".*\n([0-9a-f]+) ${HEAD}\n.*" "\\1" HASH "\n${PACKED_REFS}")
    endif()
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
