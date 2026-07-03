vcpkg_from_github(
  OUT_SOURCE_PATH SOURCE_PATH
  REPO microsoft/mimalloc
  HEAD_REF master

  # The "REF" can be a commit hash, branch name (dev3), or a version (v3.3.2).
  REF "v${VERSION}"
  # REF e2db21e9ba9fb9172b7b0aa0fe9b8742525e8774

  # The sha512 is the hash of the tar.gz bundle.
  # (To get the sha512, run `vcpkg install "mimalloc[override]" --overlay-ports=<dir of this file>` and copy the sha from the error message.)
  # (and maybe `vcpkg remove mimalloc` first to remove any previous install)
  SHA512 601bdf622d0bc7521edf0cc73d1caec9d976bcd1faa689ff48cc18a9a6a3b2294b571fc71d3266b38907bc5aad10a41d92d03d2cdde139a30b08357ee7bc25c5
)

vcpkg_check_features(OUT_FEATURE_OPTIONS FEATURE_OPTIONS
  FEATURES
    c           MI_NO_USE_CXX
    guarded     MI_GUARDED
    secure      MI_SECURE
    override    MI_OVERRIDE
    optarch     MI_OPT_ARCH
    nooptarch   MI_NO_OPT_ARCH
    optsimd     MI_OPT_SIMD
    xmalloc     MI_XMALLOC
    asm         MI_SEE_ASM
)
string(COMPARE EQUAL "${VCPKG_LIBRARY_LINKAGE}" "static" MI_BUILD_STATIC)
string(COMPARE EQUAL "${VCPKG_LIBRARY_LINKAGE}" "dynamic" MI_BUILD_SHARED)

vcpkg_cmake_configure(
  SOURCE_PATH "${SOURCE_PATH}"
  OPTIONS
    -DMI_USE_CXX=ON
    -DMI_BUILD_TESTS=OFF
    -DMI_BUILD_OBJECT=ON
    -DMI_BUILD_STATIC=${MI_BUILD_STATIC}
    -DMI_BUILD_SHARED=${MI_BUILD_SHARED}
    -DMI_INSTALL_TOPLEVEL=ON
    ${FEATURE_OPTIONS}
)

vcpkg_cmake_install()
vcpkg_copy_pdbs()

file(COPY
  "${CMAKE_CURRENT_LIST_DIR}/vcpkg-cmake-wrapper.cmake"
  "${CMAKE_CURRENT_LIST_DIR}/usage"
  DESTINATION "${CURRENT_PACKAGES_DIR}/share/${PORT}"
)
vcpkg_cmake_config_fixup(CONFIG_PATH lib/cmake/mimalloc)

if(VCPKG_LIBRARY_LINKAGE STREQUAL "dynamic")
  # todo: why is this needed?
  vcpkg_replace_string(
    "${CURRENT_PACKAGES_DIR}/include/mimalloc.h"
    "!defined(MI_SHARED_LIB)"
    "0 // !defined(MI_SHARED_LIB)"
  )
endif()
file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/debug/include")
file(REMOVE_RECURSE "${CURRENT_PACKAGES_DIR}/debug/share")

vcpkg_fixup_pkgconfig()
vcpkg_install_copyright(FILE_LIST "${SOURCE_PATH}/LICENSE")
