cmake_minimum_required(VERSION 3.12...3.31)

project(
    puff
    VERSION 2.3.0
    LANGUAGES C
    DESCRIPTION "A library for decompressing deflate with very little footprint"
    HOMEPAGE_URL "https://www.zlib.net")

option(ZLIB_PUFF_BUILD_SHARED "Enable building blast shared library" ON)
option(ZLIB_PUFF_BUILD_STATIC "Enable building blast static library" ON)
option(ZLIB_PUFF_BUILD_TESTING "Enable building tests for blast" ON)
option(ZLIB_PUFF_INSTALL "Enable installation of puff" ON)

include(GNUInstallDirs)
include(CMakePackageConfigHelpers)

if(NOT DEFINED ZLIB_BUILD_PUFF)
    if(ZLIB_PUFF_BUILD_SHARED)
        list(APPEND REQUIRED_COMPONENTS "shared")
    endif(ZLIB_PUFF_BUILD_SHARED)

    if(ZLIB_PUFF_BUILD_STATIC)
        list(APPEND REQUIRED_COMPONENTS "static")
    endif(ZLIB_PUFF_BUILD_STATIC)

    find_package(ZLIB REQUIRED COMPONENTS ${REQUIRED_COMPONENTS} CONFIG)
endif(NOT DEFINED ZLIB_BUILD_PUFF)

if(WIN32 OR CYGWIN)
    set(zlibpuff_static_suffix "s")
    set(CMAKE_DEBUG_POSTFIX "d")
endif(WIN32 OR CYGWIN)

if(ZLIB_PUFF_BUILD_TESTING)
    add_executable(zlib_puff_bin-writer bin-writer.c)

    if(${CMAKE_C_COMPILER_ID} STREQUAL "GNU" OR
       ${CMAKE_C_COMPILER_ID} STREQUAL "Clang")
        set(CFLAGS_OLD ${CMAKE_C_FLAGS})
        set(CMAKE_C_FLAGS
            ""
            CACHE STRING "" FORCE)

        if(${CMAKE_C_COMPILER_ID} STREQUAL "GNU")
            find_program(GCOV_EXECUTABLE gcov)
        endif(${CMAKE_C_COMPILER_ID} STREQUAL "GNU")

        if(${CMAKE_C_COMPILER_ID} STREQUAL "Clang")
            set(llvm_names llvm_cov)
            list(APPEND llvm_names llvm-cov)

            foreach(ver RANGE 11 99)
                list(APPEND llvm_names llvm-cov-${ver})
            endforeach(ver RANGE 11 99)

            find_program(GCOV_EXECUTABLE NAMES ${llvm_names})
            set(llvm_option "gcov")
        endif(${CMAKE_C_COMPILER_ID} STREQUAL "Clang")
    endif(${CMAKE_C_COMPILER_ID} STREQUAL "GNU" OR
          ${CMAKE_C_COMPILER_ID} STREQUAL "Clang")

      add_subdirectory(test)
endif(ZLIB_PUFF_BUILD_TESTING)

if(ZLIB_PUFF_BUILD_SHARED)
    add_library(zlib_puff_puff SHARED
                puff.c
                puff.h)

    add_library(PUFF::PUFF ALIAS zlib_puff_puff)

    if(NOT CYGWIN)
        set_target_properties(zlib_puff_puff
            PROPERTIES
                SOVERSION ${puff_VERSION_MAJOR}
                VERSION ${puff_VERSION})
    endif(NOT CYGWIN)

    set_target_properties(
        zlib_puff_puff
            PROPERTIES
                EXPORT_NAME PUFF
                OUTPUT_NAME puff)
endif(ZLIB_PUFF_BUILD_SHARED)

if(ZLIB_PUFF_BUILD_STATIC)
    add_library(zlib_puff_puffStatic STATIC
                puff.c
                puff.h)

    add_library(PUFF::PUFFSTATIC ALIAS zlib_puff_puffStatic)

    set_target_properties(zlib_puff_puffStatic
        PROPERTIES
            EXPORT_NAME PUFFSTATIC
            OUTPUT_NAME puff${zlib_puff_static_suffix})
endif(ZLIB_PUFF_BUILD_STATIC)

if(ZLIB_PUFF_INSTALL)
    if(ZLIB_PUFF_BUILD_SHARED)
        install(
            TARGETS zlib_puff_puff
            COMPONENT Runtime
            EXPORT puffSharedExport
            RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}"
            ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")
        install(
            EXPORT puffSharedExport
            FILE puff-shared.cmake
            NAMESPACE PUFF::
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/puff)

        if(MSVC)
            install(
                FILES $<TARGET_PDB_FILE:zlib_puff_puff>
                COMPONENT Development
                DESTINATION ${CMAKE_INSTALL_BINDIR}
                CONFIGURATIONS Debug OR RelWithDebInfo
                OPTIONAL)
        endif(MSVC)
    endif(ZLIB_PUFF_BUILD_SHARED)

    if(ZLIB_PUFF_BUILD_STATIC)
        install(
            TARGETS zlib_puff_puffStatic
            COMPONENT Development
            EXPORT puffStaticExport
            ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")
        install(
            EXPORT puffStaticExport
            FILE puff-static.cmake
            NAMESPACE PUFF::
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/puff)
    endif(ZLIB_PUFF_BUILD_STATIC)

    configure_package_config_file(
        ${puff_SOURCE_DIR}/puffConfig.cmake.in
        ${puff_BINARY_DIR}/puffConfig.cmake
        INSTALL_DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/puff)

    write_basic_package_version_file(
        "${puff_BINARY_DIR}/puffConfigVersion.cmake"
        VERSION "${puff_VERSION}"
        COMPATIBILITY AnyNewerVersion)

    install(FILES ${puff_BINARY_DIR}/puffConfig.cmake
                  ${puff_BINARY_DIR}/puffConfigVersion.cmake
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/puff)
    install(
        FILES puff.h
        COMPONENT Development
        DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
endif(ZLIB_PUFF_INSTALL)
