cmake_minimum_required(VERSION 3.12...3.31)

project(
    iostreamV3
    VERSION 1.0.0
    LANGUAGES CXX
    DESCRIPTION "A library for using C++ IOStreams with zlib V3"
    HOMEPAGE_URL "https://www.zlib.net")

option(ZLIB_IOSTREAM3_BUILD_SHARED "Enable building blast shared library" ON)
option(ZLIB_IOSTREAM3_BUILD_STATIC "Enable building blast static library" ON)
option(ZLIB_IOSTREAM3_BUILD_TESTING "Enable building tests for blast" ON)
option(ZLIB_IOSTREAM3_INSTALL "Enable installation of iostream" ON)

include(GNUInstallDirs)
include(CMakePackageConfigHelpers)

if(NOT DEFINED ZLIB_BUILD_IOSTREAM3)
    if(ZLIB_IOSTREAM3_BUILD_SHARED)
        list(APPEND REQUIRED_COMPONENTS "shared")
    endif(ZLIB_IOSTREAM3_BUILD_SHARED)

    if(ZLIB_IOSTREAM3_BUILD_STATIC)
        list(APPEND REQUIRED_COMPONENTS "static")
    endif(ZLIB_IOSTREAM3_BUILD_STATIC)

    find_package(ZLIB REQUIRED COMPONENTS ${REQUIRED_COMPONENTS} CONFIG)
endif(NOT DEFINED ZLIB_BUILD_IOSTREAM3)

if(WIN32 OR CYGWIN)
    set(zlibIOStream3_static_suffix "s")
    set(CMAKE_DEBUG_POSTFIX "d")
endif(WIN32 OR CYGWIN)


function(iostreamv3_findTestEnv testName)
    set(testEnv "PATH=")

    if(MSVC OR MINGW)
        set(separator "\\\;")
    else()
        set(separator ":")
    endif()

    string(APPEND testEnv "$<TARGET_FILE_DIR:zlib_iostream3_iostreamv3>${separator}")
    string(APPEND testEnv "$<TARGET_FILE_DIR:ZLIB::ZLIB>${separator}")
    string(APPEND testEnv "$ENV{PATH}")

    set_tests_properties(${testName} PROPERTIES ENVIRONMENT "${testEnv}")
endfunction(iostreamv3_findTestEnv testName)

if(ZLIB_IOSTREAM3_BUILD_SHARED)
    add_library(zlib_iostream3_iostreamv3 SHARED
                zfstream.cc
                zfstream.h)

    add_library(IOSTREAMV3::IOSTREAMV3 ALIAS zlib_iostream3_iostreamv3)

    if(NOT CYGWIN)
        set_target_properties(zlib_iostream3_iostreamv3
            PROPERTIES
                SOVERSION ${iostreamV3_VERSION_MAJOR}
                VERSION ${iostreamV3_VERSION})
    endif(NOT CYGWIN)

    set_target_properties(zlib_iostream3_iostreamv3
        PROPERTIES
            EXPORT_NAME IOSTREAMV3
            OUTPUT_NAME iostream3)

    target_link_libraries(zlib_iostream3_iostreamv3
        PUBLIC ZLIB::ZLIB)

    if(ZLIB_IOSTREAM3_BUILD_TESTING)
        enable_testing()

        add_executable(zlib_iostream3_test test.cc zfstream.h)

        target_link_libraries(zlib_iostream3_test
            PRIVATE zlib_iostream3_iostreamv3)

        add_test(NAME zlib_iostream3_test COMMAND zlib_iostream3_test)

        set_tests_properties(zlib_iostream3_test
            PROPERTIES
                FIXTURES_REQUIRED zlib_iostream3_cleanup)

        if(MSVC
           OR MSYS
           OR MINGW
           OR CYGWIN)
            iostreamv3_findtestenv(zlib_iostream3_test)
        endif(
            MSVC
            OR MSYS
            OR MINGW
            OR CYGWIN)
    endif(ZLIB_IOSTREAM3_BUILD_TESTING)
endif(ZLIB_IOSTREAM3_BUILD_SHARED)

if(ZLIB_IOSTREAM3_BUILD_STATIC)
    add_library(zlib_iostream3_iostreamv3Static STATIC
                zfstream.cc
                zfstream.h)

    add_library(IOSTREAMV3::IOSTREAMV3STATIC
        ALIAS zlib_iostream3_iostreamv3Static)

    target_link_libraries(zlib_iostream3_iostreamv3Static
        PUBLIC ZLIB::ZLIBSTATIC)

    set_target_properties(zlib_iostream3_iostreamv3Static
        PROPERTIES
            EXPORT_NAME IOSTREAMV3STATIC
            OUTPUT_NAME iostream3${zlib_IOStream3_static_suffix})

    if(ZLIB_IOSTREAM3_BUILD_TESTING)
        enable_testing()

        add_executable(zlib_iostream3_testStatic test.cc zfstream.h)

        target_link_libraries(zlib_iostream3_testStatic
            PRIVATE zlib_iostream3_iostreamv3Static)

        add_test(NAME zlib_iostream3_testStatic
                 COMMAND zlib_iostream3_testStatic)

        set_tests_properties(zlib_iostream3_testStatic
            PROPERTIES
                FIXTURES_REQUIRED zlib_iostream3_cleanup)
    endif(ZLIB_IOSTREAM3_BUILD_TESTING)
endif(ZLIB_IOSTREAM3_BUILD_STATIC)

if(ZLIB_IOSTREAM3_BUILD_TESTING)
    add_test(NAME zlib_iostream3_cleanup COMMAND ${CMAKE_COMMAND} -E rm
        ${CMAKE_CURRENT_BINARY_DIR}/test1.txt.gz
        ${CMAKE_CURRENT_BINARY_DIR}/test2.txt.gz)

    set_tests_properties(zlib_iostream3_cleanup
        PROPERTIES
            FIXTURES_CLEANUP zlib_iostream3_cleanup)

        add_subdirectory(test)
endif(ZLIB_IOSTREAM3_BUILD_TESTING)

if(ZLIB_IOSTREAM3_INSTALL)
    if(ZLIB_IOSTREAM3_BUILD_SHARED)
        install(
            TARGETS zlib_iostream3_iostreamv3
            COMPONENT Runtime
            EXPORT zlibiostream3SharedExport
            RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}"
            ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")
        install(
            EXPORT zlibiostream3SharedExport
            FILE iostreamv3-shared.cmake
            NAMESPACE IOSTREAMV3::
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/iostreamv3)

        if(MSVC)
            install(
                FILES $<TARGET_PDB_FILE:zlib_iostream3_iostreamv3>
                COMPONENT Development
                DESTINATION ${CMAKE_INSTALL_BINDIR}
                CONFIGURATIONS Debug OR RelWithDebInfo
                OPTIONAL)
        endif(MSVC)
    endif(ZLIB_IOSTREAM3_BUILD_SHARED)

    if(ZLIB_IOSTREAM3_BUILD_STATIC)
        install(
            TARGETS zlib_iostream3_iostreamv3Static
            COMPONENT Development
            EXPORT iostream3StaticExport
            ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")
        install(
            EXPORT iostream3StaticExport
            FILE iostreamv3-static.cmake
            NAMESPACE IOSTREAMV3::
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/iostreamv3)
    endif(ZLIB_IOSTREAM3_BUILD_STATIC)

    configure_package_config_file(
        ${iostreamV3_SOURCE_DIR}/iostream3Config.cmake.in
        ${iostreamV3_BINARY_DIR}/iostreamv3Config.cmake
        INSTALL_DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/iostreamv3)

    write_basic_package_version_file(
        "${iostreamV3_BINARY_DIR}/iostreamv3ConfigVersion.cmake"
        VERSION "${iostream3_VERSION}"
        COMPATIBILITY AnyNewerVersion)

    install(FILES ${iostreamV3_BINARY_DIR}/iostreamv3Config.cmake
                  ${iostreamV3_BINARY_DIR}/iostreamv3ConfigVersion.cmake
            DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/iostreamv3)
    install(
        FILES zfstream.h
        COMPONENT Development
        DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
endif(ZLIB_IOSTREAM3_INSTALL)
