if(CMAKE_VERSION VERSION_GREATER_EQUAL 3.10)
  include_guard(GLOBAL)
endif()

if(CMAKE_VERSION VERSION_GREATER_EQUAL 3.14)
  set(BLAKE3_CXX_COMPILER_FRONTEND_VARIANT "${CMAKE_CXX_COMPILER_FRONTEND_VARIANT}")
else()
  # Get the C++ compiler name without extension
  get_filename_component(BLAKE3_CMAKE_CXX_COMPILER_NAME "${CMAKE_CXX_COMPILER}" NAME_WE)
  # Strip any trailing versioning from the C++ compiler name
  string(REGEX MATCH "^.*(clang\\+\\+|clang-cl)" BLAKE3_CMAKE_CXX_COMPILER_NAME "${BLAKE3_CMAKE_CXX_COMPILER_NAME}")
  # Guess the frontend variant from the C++ compiler name
  if(CMAKE_CXX_COMPILER_ID STREQUAL "Clang" AND BLAKE3_CMAKE_CXX_COMPILER_NAME STREQUAL "clang-cl")
    set(BLAKE3_CXX_COMPILER_FRONTEND_VARIANT "MSVC")
  elseif(CMAKE_CXX_COMPILER_ID STREQUAL "MSVC")
    set(BLAKE3_CXX_COMPILER_FRONTEND_VARIANT "MSVC")
  else()
    set(BLAKE3_CXX_COMPILER_FRONTEND_VARIANT "GNU")
  endif()
  unset(BLAKE3_CMAKE_CXX_COMPILER_NAME)
endif()
