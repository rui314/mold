vcpkg_from_github(
  OUT_SOURCE_PATH SOURCE_PATH
  REPO microsoft/mimalloc
  HEAD_REF master

  # The "REF" can be a commit hash, branch name (dev2), or a version (v2.2.1).
  # REF "v${VERSION}"
  REF 6a89f8554eaab8d8d00e17b5b09f79e1d8dbf61b

  # The sha512 is the hash of the tar.gz bundle.
  # (To get the sha512, run `vcpkg install mimalloc[override] --overlay-ports=<dir of this file>` and copy the sha from the error message.)
  SHA512 32b87a3195efcc558b83a546348a8fb544fed335cdd6c9f8e7e9d0e8e64540fdcf1f4aa57fd0e783b78731518f4810292b832227d7e7665bf8426f1e6ce96f9d
)

vcpkg_check_features(OUT_FEATURE_OPTIONS FEATURE_OPTIONS
  FEATURES
    c           MI_NO_USE_CXX
    guarded     MI_GUARDED
    secure      MI_SECURE
    override    MI_OVERRIDE
    xmalloc     MI_XMALLOC
    asm         MI_SEE_ASM
)
string(COMPARE EQUAL "${VCPKG_LIBRARY_LINKAGE}" "static" MI_BUILD_STATIC)
string(COMPARE EQUAL "${VCPKG_LIBRARY_LINKAGE}" "dynamic" MI_BUILD_SHARED)

vcpkg_cmake_configure(
  SOURCE_PATH "${SOURCE_PATH}"
  OPTIONS_RELEASE
    -DMI_OPT_ARCH=ON
  OPTIONS
    -DMI_USE_CXX=ON
    -DMI_BUILD_TESTS=OFF
    -DMI_BUILD_OBJECT=ON
    ${FEATURE_OPTIONS}
    -DMI_BUILD_STATIC=${MI_BUILD_STATIC}
    -DMI_BUILD_SHARED=${MI_BUILD_SHARED}
    -DMI_INSTALL_TOPLEVEL=ON
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
