find_package(TBB 2021.11.0 QUIET)

if(CMAKE_VERSION VERSION_GREATER_EQUAL 3.11)
  include(FetchContent)

  if(NOT TBB_FOUND AND BLAKE3_FETCH_TBB)
    set(CMAKE_C_STANDARD 99)
    set(CMAKE_C_EXTENSIONS OFF)

    set(CMAKE_CXX_STANDARD 20)
    set(CMAKE_CXX_EXTENSIONS ON)

    option(TBB_TEST OFF "")
    option(TBBMALLOC_BUILD OFF "")

    mark_as_advanced(TBB_TEST)
    mark_as_advanced(TBBMALLOC_BUILD)

    FetchContent_Declare(
      TBB
      GIT_REPOSITORY https://github.com/uxlfoundation/oneTBB
      GIT_TAG 0c0ff192a2304e114bc9e6557582dfba101360ff # v2022.0.0
      GIT_SHALLOW TRUE
    )

    FetchContent_MakeAvailable(TBB)
  endif()
endif()
