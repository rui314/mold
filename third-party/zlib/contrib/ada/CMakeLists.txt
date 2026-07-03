cmake_minimum_required(VERSION 3.12...3.31)

set(CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake/Modules")

project(
    zlibAda
    VERSION 1.0.0
    LANGUAGES C ADA
    DESCRIPTION "A library for creating zipfiles based in zlib"
    HOMEPAGE_URL "https://www.zlib.net")

option(ZLIB_ADA_BUILD_SHARED "Enable building ada bindings shared library" ON)
option(ZLIB_ADA_BUILD_STATIC "Enable building ada bindings static library" ON)
option(ZLIB_ADA_BUILD_TESTING "Enable building tests for ada bindings library" ON)

if(WIN32 OR CYGWIN)
    set(zlib_Ada_static_suffix "s")
    set(CMAKE_DEBUG_POSTFIX "d")
endif(WIN32 OR CYGWIN)

if(NOT DEFINED ZLIB_BUILD_ADA)
    if(ZLIB_ADA_BUILD_SHARED)
        list(APPEND REQUIRED_COMPONENTS "shared")
    endif(ZLIB_ADA_BUILD_SHARED)

    if(ZLIB_ADA_BUILD_STATIC)
        list(APPEND REQUIRED_COMPONENTS "static")
    endif(ZLIB_ADA_BUILD_STATIC)

    find_package(ZLIB REQUIRED COMPONENTS ${REQUIRED_COMPONENTS} CONFIG)
endif(NOT DEFINED ZLIB_BUILD_ADA)

function(ZLIB_ADA_findTestEnv testName)
    set(testEnv "PATH=")

    if(MSVC OR MINGW)
        set(separator "\\\;")
    else()
        set(separator ":")
    endif()

    string(APPEND testEnv "$<TARGET_FILE_DIR:ZLIB::ZLIB>${separator}")
    string(APPEND testEnv "$ENV{PATH}")

    set_tests_properties(${testName} PROPERTIES ENVIRONMENT "${testEnv}")
endfunction(ZLIB_ADA_findTestEnv testName)

if(ZLIB_ADA_BUILD_SHARED)
    ada_add_library(zlib_ada_Ada SHARED
        zlib-thin.adb
        zlib.adb)

    set_target_properties(zlib_ada_Ada
        PROPERTIES OUTPUT_NAME zlib-ada)

    target_link_libraries(zlib_ada_Ada
        INTERFACE ZLIB::ZLIB)

    ada_add_library(zlib_ada_streams SHARED
        zlib-streams.adb)

    target_link_libraries(zlib_ada_streams
        PUBLIC
            zlib_ada_Ada)

    ada_find_ali(zlib_ada_streams)

    if(ZLIB_ADA_BUILD_TESTING)
        enable_testing()
        ada_add_executable(zlib_ada_test test.adb)

        target_link_libraries(zlib_ada_test
            PRIVATE
                zlib_ada_Ada
                zlib_ada_streams)

        ada_find_ali(zlib_ada_test)

        add_test(NAME zlib_ada_ada-test COMMAND zlib_ada_test)
        set_tests_properties(zlib_ada_ada-test PROPERTIES FIXTURES_REQUIRED zlib_ada_cleanup)

        if(MSVC
           OR MSYS
           OR MINGW
           OR CYGWIN)
            zlib_ada_findtestenv(zlib_ada_ada-test)
        endif(
            MSVC
            OR MSYS
            OR MINGW
            OR CYGWIN)

        ada_add_executable(zlib_ada_buffer_demo buffer_demo.adb)

        target_link_libraries(zlib_ada_buffer_demo
            PRIVATE
                zlib_ada_Ada)

        ada_find_ali(zlib_ada_buffer_demo)

        add_test(NAME zlib_ada_buffer-demo COMMAND zlib_ada_buffer_demo)

        if(MSVC
           OR MSYS
           OR MINGW
           OR CYGWIN)
            zlib_ada_findtestenv(zlib_ada_buffer-demo)
        endif(
            MSVC
            OR MSYS
            OR MINGW
            OR CYGWIN)

        ada_add_executable(zlib_ada_mtest mtest.adb)

        target_link_libraries(zlib_ada_mtest
            PRIVATE
                zlib_ada_Ada)

        ada_find_ali(zlib_ada_mtest)

        #Not adding test as this is an endless-loop

        ada_add_executable(zlib_ada_read read.adb)

        target_link_libraries(zlib_ada_read
            PRIVATE
                zlib_ada_Ada)

        ada_find_ali(zlib_ada_read)

        add_test(NAME zlib_ada_read COMMAND zlib_ada_read)

        if(MSVC
           OR MSYS
           OR MINGW
           OR CYGWIN)
            zlib_ada_findtestenv(zlib_ada_read)
        endif(
            MSVC
            OR MSYS
            OR MINGW
            OR CYGWIN)
    endif(ZLIB_ADA_BUILD_TESTING)
endif(ZLIB_ADA_BUILD_SHARED)

if(ZLIB_ADA_BUILD_STATIC)
    ada_add_library(zlib_ada_AdaStatic STATIC
        zlib-thin.adb
        zlib.adb)

    target_link_libraries(zlib_ada_AdaStatic
        INTERFACE ZLIB::ZLIBSTATIC)

    set_target_properties(zlib_ada_AdaStatic
        PROPERTIES OUTPUT_NAME zlib-ada${zlib_Ada_static_suffix})

    ada_add_library(zlib_ada_streamsStatic STATIC
        zlib-streams.adb)

    target_link_libraries(zlib_ada_streamsStatic
        PUBLIC
            zlib_ada_AdaStatic)

    ada_find_ali(zlib_ada_streamsStatic)

    if(ZLIB_ADA_BUILD_TESTING)
        enable_testing()
        ada_add_executable(zlib_ada_testStatic test.adb)

        target_link_libraries(zlib_ada_testStatic
            PRIVATE
                zlib_ada_AdaStatic
                zlib_ada_streamsStatic)

        ada_find_ali(zlib_ada_testStatic)

        add_test(NAME zlib_ada_testStatic COMMAND zlib_ada_testStatic)
        set_tests_properties(zlib_ada_testStatic PROPERTIES FIXTURES_REQUIRED zlib_ada_cleanup)

        ada_add_executable(zlib_ada_buffer-demoStatic buffer_demo.adb)

        target_link_libraries(zlib_ada_buffer-demoStatic
            PRIVATE
                zlib_ada_AdaStatic)

        ada_find_ali(zlib_ada_buffer-demoStatic)

        add_test(NAME zlib_ada_buffer-demoStatic COMMAND zlib_ada_buffer-demoStatic)

        ada_add_executable(zlib_ada_mtestStatic mtest.adb)

        target_link_libraries(zlib_ada_mtestStatic
            PRIVATE
                zlib_ada_AdaStatic)

        ada_find_ali(zlib_ada_mtestStatic)

        # Not adding test as this is an endless-loop

        ada_add_executable(zlib_ada_readStatic read.adb)

        target_link_libraries(zlib_ada_readStatic
            PRIVATE
                zlib_ada_AdaStatic)

        ada_find_ali(zlib_ada_readStatic)

        add_test(NAME zlib_ada_readStatic COMMAND zlib_ada_readStatic)
    endif(ZLIB_ADA_BUILD_TESTING)
endif(ZLIB_ADA_BUILD_STATIC)

if(ZLIB_ADA_BUILD_TESTING)
    add_test(NAME zlib_ada_cleanup COMMAND ${CMAKE_COMMAND} -E rm ${CMAKE_CURRENT_BINARY_DIR}/testzlib.in
        ${CMAKE_CURRENT_BINARY_DIR}/testzlib.out ${CMAKE_CURRENT_BINARY_DIR}/testzlib.zlb)
    set_tests_properties(zlib_ada_cleanup PROPERTIES FIXTURES_CLEANUP zlib_ada_cleanup)
endif(ZLIB_ADA_BUILD_TESTING)
