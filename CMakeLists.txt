cmake_minimum_required(VERSION 3.10)
project(mold VERSION 1.0.0)

SET(BUILD_SHARED_LIBS OFF CACHE INTERNAL "BUILD_SHARED_LIBS")

add_definitions(-D__TBB_RESUMABLE_TASKS_USE_THREADS=1)

find_package(Threads)
include(cmake/pkg.cmake)

file(GLOB MOLD_SRC_FILES ./*.cc elf/*.cc macho/*.cc)
add_executable(mold ${MOLD_SRC_FILES})
target_compile_features(mold PRIVATE cxx_std_20)
target_link_libraries(mold crypto tbb mimalloc-static mimalloc-static zlibstatic xxhash ${CMAKE_THREAD_LIBS_INIT})
target_compile_definitions(mold PRIVATE
  MOLD_VERSION="1.0.0"
  __TBB_RESUMABLE_TASKS_USE_THREADS=1
)
target_link_options(mold PRIVATE -flto)
target_compile_options(mold PRIVATE -fPIE -fno-unwind-tables -fno-asynchronous-unwind-tables)

if (${CMAKE_BUILD_TYPE} STREQUAL Release)
  add_custom_command(
    TARGET mold
    POST_BUILD
    COMMAND ${CMAKE_STRIP} ${CMAKE_BINARY_DIR}/mold
  )
endif()