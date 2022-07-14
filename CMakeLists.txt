cmake_minimum_required(VERSION 3.14)
project(mold VERSION 1.3.0)

include(CMakeDependentOption)

# FIXME: this is for parity with the makefiles that install directly to <prefix>/lib
# NOTE: defining this before including GNUInstallDirs makes its platform detection a noop
set(CMAKE_INSTALL_LIBDIR lib CACHE PATH "")
include(GNUInstallDirs)

# Mold build options
cmake_dependent_option(MOLD_USE_MIMALLOC "Use mimalloc" ON
    "NOT APPLE;NOT ANDROID" OFF)
cmake_dependent_option(MOLD_USE_SYSTEM_MIMALLOC "Use system or vendored mimalloc" ON
    MOLD_USE_MIMALLOC OFF)
option(MOLD_USE_SYSTEM_TBB "Use system or vendored TBB" ON)

# Setup mimalloc
if(MOLD_USE_MIMALLOC)
    if(MOLD_USE_SYSTEM_MIMALLOC)
        find_package(mimalloc REQUIRED)
        target_compile_definitions(mimalloc INTERFACE USE_SYSTEM_MIMALLOC)
    else()
        function(mold_add_mimalloc)
            option(MI_BUILD_TESTS "Build test executables" OFF)
            add_subdirectory(third-party/mimalloc EXCLUDE_FROM_ALL)
            target_compile_definitions(mimalloc PRIVATE MI_USE_ENVIRON=0)
        endfunction()

        mold_add_mimalloc()
    endif()
endif()

# Setup TBB
if(MOLD_USE_SYSTEM_TBB)
    find_package(TBB REQUIRED)
else()
    function(mold_add_tbb)
        set(BUILD_SHARED_LIBS OFF)
        set(TBB_TEST OFF CACHE INTERNAL "")
        set(TBB_STRICT OFF CACHE INTERNAL "")
        add_subdirectory(third-party/tbb EXCLUDE_FROM_ALL)
        target_compile_definitions(tbb PRIVATE __TBB_DYNAMIC_LOAD_ENABLED=0)
    endfunction()

    mold_add_tbb()
endif()

add_library(mold-wrapper SHARED)

# Remove the default `lib` prefix
set_target_properties(mold-wrapper PROPERTIES PREFIX "")
target_link_libraries(mold-wrapper PRIVATE ${CMAKE_DL_LIBS})
target_sources(mold-wrapper PRIVATE elf/mold-wrapper.c)

add_executable(mold)
target_compile_features(mold PRIVATE cxx_std_20)
target_include_directories(mold SYSTEM PRIVATE third-party)

if(NOT MSVC)
    include(CheckLibraryExists)
    check_library_exists(m pow "" LIBM_FOUND)

    if(LIBM_FOUND)
        target_link_libraries(mold PRIVATE m)
    endif()
endif()

find_package(ZLIB REQUIRED)
target_link_libraries(mold PRIVATE ZLIB::ZLIB TBB::tbb ${CMAKE_DL_LIBS})

if(MOLD_USE_MIMALLOC)
    target_link_libraries(mold PRIVATE mimalloc)
endif()

if(NOT APPLE)
    find_package(OpenSSL REQUIRED COMPONENTS Crypto)
    target_link_libraries(mold PRIVATE OpenSSL::Crypto)
endif()

if(CMAKE_SYSTEM_PROCESSOR MATCHES riscv64
    OR CMAKE_SYSTEM_PROCESSOR MATCHES armv6)
    target_link_libraries(mold PRIVATE atomic)
endif()

set_property(SOURCE main.cc elf/lto.cc macho/output-chunks.cc APPEND PROPERTY
    COMPILE_DEFINITIONS "MOLD_VERSION=\"${CMAKE_PROJECT_VERSION}\"")

if(EXISTS .git/HEAD)
    file(STRINGS .git/HEAD GIT_HASH)

    if(GIT_HASH MATCHES "ref: ")
        string(REPLACE "ref: " "" GIT_HASH "${GIT_HASH}")
        file(STRINGS .git/${GIT_HASH} GIT_HASH)
    endif()

    set_property(SOURCE main.cc APPEND PROPERTY
        COMPILE_DEFINITIONS "GIT_HASH=\"${GIT_HASH}\"")
endif()

target_sources(mold PRIVATE
    compress.cc
    demangle.cc
    filepath.cc
    glob.cc
    hyperloglog.cc
    main.cc
    multi-glob.cc
    perf.cc
    strerror.cc
    tar.cc
    uuid.cc
    elf/arch-arm32.cc
    elf/arch-arm64.cc
    elf/arch-i386.cc
    elf/arch-riscv64.cc
    elf/arch-x86-64.cc
    elf/cmdline.cc
    elf/dwarf.cc
    elf/gc-sections.cc
    elf/icf.cc
    elf/input-files.cc
    elf/input-sections.cc
    elf/linker-script.cc
    elf/lto.cc
    elf/main.cc
    elf/mapfile.cc
    elf/output-chunks.cc
    elf/passes.cc
    elf/relocatable.cc
    elf/subprocess.cc
    macho/arch-arm64.cc
    macho/arch-x86-64.cc
    macho/cmdline.cc
    macho/dead-strip.cc
    macho/input-files.cc
    macho/input-sections.cc
    macho/lto.cc
    macho/main.cc
    macho/mapfile.cc
    macho/output-chunks.cc
    macho/tapi.cc
    macho/yaml.cc
    third-party/rust-demangle/rust-demangle.c)

if(NOT CMAKE_SKIP_INSTALL_RULES)
    install(TARGETS mold)
    install(TARGETS mold-wrapper DESTINATION ${CMAKE_INSTALL_LIBDIR}/mold)
    install(CODE "\
        file(REAL_PATH \${CMAKE_INSTALL_PREFIX} INSTALL_PREFIX)
        file(RELATIVE_PATH\
            REL_SYMLINK\
            \${INSTALL_PREFIX}/${CMAKE_INSTALL_BINDIR}/mold\
            \${INSTALL_PREFIX}/${CMAKE_INSTALL_LIBEXECDIR}/mold)\n\
        execute_process(\
            COMMAND ${CMAKE_COMMAND} -E make_directory\
            \${INSTALL_PREFIX}/${CMAKE_INSTALL_LIBEXECDIR}/mold)\n\
        execute_process(\
            COMMAND ${CMAKE_COMMAND} -E create_symlink\
            \${REL_SYMLINK}\
            \${INSTALL_PREFIX}/${CMAKE_INSTALL_LIBEXECDIR}/mold/ld)\n\
        execute_process(\
            COMMAND ${CMAKE_COMMAND} -E create_symlink\
            mold\
            \${INSTALL_PREFIX}/${CMAKE_INSTALL_BINDIR}/ld.mold)\n\
        execute_process(\
            COMMAND ${CMAKE_COMMAND} -E create_symlink\
            mold\
            \${INSTALL_PREFIX}/${CMAKE_INSTALL_BINDIR}/ld64.mold)")
endif()
