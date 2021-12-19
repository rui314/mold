cmake_minimum_required(VERSION 3.10)
project(deps)

add_subdirectory(libressl EXCLUDE_FROM_ALL)
add_subdirectory(mimalloc EXCLUDE_FROM_ALL)
add_subdirectory(xxHash EXCLUDE_FROM_ALL)
add_subdirectory(zlib EXCLUDE_FROM_ALL)
add_subdirectory(oneTBB EXCLUDE_FROM_ALL)
