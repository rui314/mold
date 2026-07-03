include(${CMAKE_ROOT}/Modules/CMakeDetermineCompiler.cmake)

# Load system-specific compiler preferences for this language.
include(Platform/${CMAKE_SYSTEM_NAME}-Determine-Ada OPTIONAL)
include(Platform/${CMAKE_SYSTEM_NAME}-Ada OPTIONAL)

if(NOT CMAKE_ADA_COMPILER_NAMES)
    set(CMAKE_ADA_COMPILER_NAMES gnat)

    foreach(ver RANGE 11 99)
        list(APPEND CMAKE_ADA_COMPILER_NAMES gnat-${ver})
    endforeach(ver RANGE 11 99)
endif(NOT CMAKE_ADA_COMPILER_NAMES)

if(NOT CMAKE_ADA_COMPILER)
    set(CMAKE_ADA_COMPILER_INIT NOTFOUND)
    _cmake_find_compiler(ADA)
else(NOT CMAKE_REAL_ADA_COMPILER)
    _cmake_find_compiler_path(ADA)
endif(NOT CMAKE_ADA_COMPILER)

mark_as_advanced(CMAKE_ADA_COMPILER)
set(CMAKE_ADA_COMPILER_ID "GNU")
set(CMAKE_ADA_BINDER_HELPER "${CMAKE_COMMAND} -P ${CMAKE_CURRENT_SOURCE_DIR}/cmake/binder_helper.cmake")
set(CMAKE_ADA_COMPILER_HELPER "${CMAKE_COMMAND} -P ${CMAKE_CURRENT_SOURCE_DIR}/cmake/compile_helper.cmake")
set(CMAKE_ADA_EXE_LINK_HELPER "${CMAKE_COMMAND} -P ${CMAKE_CURRENT_SOURCE_DIR}/cmake/exe_link_helper.cmake")
set(CMAKE_ADA_SHARED_LINK_HELPER "${CMAKE_COMMAND} -P ${CMAKE_CURRENT_SOURCE_DIR}/cmake/shared_link_helper.cmake")
set(CMAKE_ADA_STATIC_LINK_HELPER "${CMAKE_COMMAND} -P ${CMAKE_CURRENT_SOURCE_DIR}/cmake/static_link_helper.cmake")

configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/cmake/Modules/CMakeADACompiler.cmake.in
    ${CMAKE_PLATFORM_INFO_DIR}/CMakeADACompiler.cmake
    @ONLY)
