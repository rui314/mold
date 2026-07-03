cmake_minimum_required(VERSION 3.12...3.31)

project(
    zlib1-dll
    LANGUAGES C
    VERSION 1.3.2
    HOMEPAGE_URL "https://zlib.net/"
    DESCRIPTION "zlib1.dll is the legacy DLL with zlib and minizip")

# ============================================================================
# configuration
# ============================================================================

if(NOT WIN32)
    message(FATAL_ERROR "This creates zlib1.<DLL>, Nothing else")
endif(NOT WIN32)

option(ENABLE_BZIP2 "Build with bzip2 support" OFF)
set(CMAKE_WINDOWS_EXPORT_ALL_SYMBOLS TRUE)

include(CheckCSourceCompiles)
include(CheckFunctionExists)
include(CheckIncludeFile)
include(CMakePackageConfigHelpers)
include(CheckTypeSize)
include(CPack)
include(GNUInstallDirs)

if(NOT ZLIB_CONF_WRITTEN)
    set(CONF_OUT_FILE ${zlib1-dll_BINARY_DIR}/zconf.h.cmakein)
    file(READ ../../zconf.h ZCONF_CONTENT LIMIT 245)
    file(WRITE ${CONF_OUT_FILE} ${ZCONF_CONTENT})
    file(APPEND ${CONF_OUT_FILE} "#cmakedefine Z_PREFIX 1\n")
    file(APPEND ${CONF_OUT_FILE} "#cmakedefine HAVE_STDARG_H 1\n")
    file(APPEND ${CONF_OUT_FILE} "#cmakedefine HAVE_UNISTD_H 1\n")
    file(READ ../../zconf.h ZCONF_CONTENT OFFSET 244)
    set(FIRST_ITEM TRUE)

    foreach(item IN LISTS ZCONF_CONTENT)
        if(FIRST_ITEM)
            string(APPEND OUT_CONTENT ${item})
            set(FIRST_ITEM FALSE)
        else(FIRST_ITEM)
            string(APPEND OUT_CONTENT "\;" ${item})
        endif(FIRST_ITEM)
    endforeach(item IN LISTS ${ZCONF_CONTENT})

    file(APPEND ${CONF_OUT_FILE} ${OUT_CONTENT})
    set(ZLIB_CONF_WRITTEN
        TRUE
        CACHE BOOL "zconf.h.cmakein was created")
    mark_as_advanced(ZLIB_CONF_WRITTEN)
endif(NOT ZLIB_CONF_WRITTEN)

if(ENABLE_BZIP2)
    find_package(BZip2 REQUIRED)
endif(ENABLE_BZIP2)

#
# Check for fopen64
#
check_function_exists(fopen64 HAVE_FOPEN64)

#
# Check to see if we have large file support
#
set(CMAKE_REQUIRED_DEFINITIONS -D_LARGEFILE64_SOURCE=1)
check_type_size(off64_t OFF64_T)
unset(CMAKE_REQUIRED_DEFINITIONS) # clear variable

#
# Check for fseeko
#
check_function_exists(fseeko HAVE_FSEEKO)

#
# Check for stdarg.h
#
check_include_file(stdarg.h HAVE_STDARG_H)

#
# Check for unistd.h
#
check_include_file(unistd.h HAVE_UNISTD_H)

#
# Check visibility attribute is supported
#
if(MSVC)
    set(CMAKE_REQUIRED_FLAGS "-WX")
else(MSVC)
    set(CMAKE_REQUIRED_FLAGS "-Werror")
endif(MSVC)

check_c_source_compiles(
    "
        #include <stdlib.h>
        static void f(void) __attribute__ ((visibility(\"hidden\")));
        int main(void) {return 0;}
    "
    HAVE___ATTR__VIS_HIDDEN)

unset(CMAKE_COMPILE_FLAGS)
configure_file(${zlib1-dll_BINARY_DIR}/zconf.h.cmakein ${zlib1-dll_BINARY_DIR}/zconf.h)

# ============================================================================
# zlib1-dll
# ============================================================================
set(ZLIB1-DLL_PUBLIC_HDRS
    ${zlib1-dll_BINARY_DIR}/zconf.h
    ../../zlib.h
    ../minizip/crypt.h
    ../minizip/ints.h
    ../minizip/ioapi.h
    ../minizip/mztools.h
    ../minizip/unzip.h
    ../minizip/zip.h)

set(ZLIB1-DLL_PRIVATE_HDRS
    ../../crc32.h
    ../../deflate.h
    ../../gzguts.h
    ../../inffast.h
    ../../inffixed.h
    ../../inflate.h
    ../../inftrees.h
    ../../trees.h
    ../../zutil.h)

set(ZLIB1-DLL_SRCS
    ../../adler32.c
    ../../compress.c
    ../../crc32.c
    ../../deflate.c
    ../../gzclose.c
    ../../gzlib.c
    ../../gzread.c
    ../../gzwrite.c
    ../../inflate.c
    ../../infback.c
    ../../inftrees.c
    ../../inffast.c
    ../../trees.c
    ../../uncompr.c
    ../../win32/zlib1.rc
    ../../zutil.c
    ../minizip/ioapi.c
    ../minizip/mztools.c
    ../minizip/unzip.c
    ../minizip/zip.c)

add_library(zlib1 SHARED ${ZLIB1-DLL_SRCS}
                         ${ZLIB1-DLL_PUBLIC_HDRS}
                         ${ZLIB1-DLL_PRIVATE_HDRS})

#taget_include_directories doesn't like relative paths
include_directories(../../
                    ../minizip
                    ${CMAKE_CURRENT_BINARY_DIR})

target_compile_definitions(zlib1
    PRIVATE ZLIB_BUILD
            $<$<BOOL:NOT:${HAVE_FSEEKO}>:NO_FSEEKO>
            $<$<BOOL:${HAVE_UNISTD_H}>:HAVE_UNISTD_H=1>
            $<$<BOOL:${HAVE___ATTR__VIS_HIDDEN}>:HAVE_HIDDEN>
            $<$<BOOL:${MSVC}>:_CRT_SECURE_NO_DEPRECATE>
            $<$<BOOL:${MSVC}>:_CRT_NONSTDC_NO_DEPRECATE>
    PUBLIC $<$<BOOL:${HAVE_OFF64_T}>:_LARGEFILE64_SOURCE=1>
           $<$<BOOL:${BZIP2_FOUND}>:HAVE_BZIP2=1>
           $<$<BOOL:NOT:${HAVE_FOPEN64}>:USE_FILE32API=1>)

target_link_libraries(zlib1
    PUBLIC $<$<BOOL:${BZIP2_FOUND}>:BZip2::BZip2>)

if(NOT CYGWIN)
    set_target_properties(zlib1 PROPERTIES
        SOVERSION ${zlib1-dll_VERSION_MAJOR}
        VERSION ${zlib1-dll_VERSION})
endif(NOT CYGWIN)

set_target_properties(zlib1 PROPERTIES
    DEFINE_SYMBOL ZLIB_DLL)

install(
    TARGETS zlib1
    ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}"
    RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}"
    LIBRARY DESTINATION "${CMAKE_INSTALL_LIBDIR}")

install(
    FILES ${ZLIB1-DLL_PUBLIC_HDRS}
    DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")

install(
    FILES ../../LICENSE
    DESTINATION "${CMAKE_INSTALL_DOCDIR}/zlib1-dll")
