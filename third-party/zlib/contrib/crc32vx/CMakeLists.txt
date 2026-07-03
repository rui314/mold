# check if we compile for IBM s390x
#
CHECK_C_SOURCE_COMPILES("
#ifndef __s390x__
    #error
#endif
int main() {return 0;}
" HAS_S390X_SUPPORT)

#
# Check for IBM S390X - VX extensions
#
if(ZLIB_WITH_CRC32VX AND HAS_S390X_SUPPORT)
    # preset the compiler specific flags
    if (CMAKE_C_COMPILER_ID STREQUAL "Clang")
        set(VGFMAFLAG "-fzvector")
    else()
        set(VGFMAFLAG "-mzarch")
    endif(CMAKE_C_COMPILER_ID STREQUAL "Clang")

    set(S390X_VX_TEST
        "#ifndef __s390x__ \n\
        #error \n\
        #endif \n\
        #include <vecintrin.h> \n\
        int main(void) { \
           unsigned long long a __attribute__((vector_size(16))) = { 0 }; \
           unsigned long long b __attribute__((vector_size(16))) = { 0 }; \
           unsigned char c __attribute__((vector_size(16))) = { 0 }; \
           c = vec_gfmsum_accum_128(a, b, c); \
           return c[0]; \
        }")

    # cflags already contains a valid march
    set(CMAKE_REQUIRED_FLAGS "${VGFMAFLAG}")
    check_c_source_compiles("${S390X_VX_TEST}" HAS_S390X_VX_SUPPORT)
    unset(CMAKE_REQUIRED_FLAGS)

    # or set march for our compile units
    if(NOT HAS_S390X_VX_SUPPORT)
        set(CMAKE_REQUIRED_FLAGS "${VGFMAFLAG} -march=z13")
        check_c_source_compiles("${S390X_VX_TEST}" HAS_Z13_S390X_VX_SUPPORT)
        unset(CMAKE_REQUIRED_FLAGS )
        list(APPEND VGFMAFLAG "-march=z13")
    endif(NOT HAS_S390X_VX_SUPPORT)

    # prepare compiling for s390x
    if(HAS_S390X_VX_SUPPORT OR HAS_Z13_S390X_VX_SUPPORT)
        if(ZLIB_BUILD_SHARED)
            target_sources(zlib
                PRIVATE
                    crc32_vx.c
                    crc32_vx_hooks.h)
            target_compile_definitions(zlib PUBLIC -DHAVE_S390X_VX=1)
        endif(ZLIB_BUILD_SHARED)
        if(ZLIB_BUILD_STATIC)
            target_sources(zlibstatic
                PRIVATE
                    crc32_vx.c
                    crc32_vx_hooks.h)
            target_compile_definitions(zlibstatic PUBLIC -DHAVE_S390X_VX=1)
        endif(ZLIB_BUILD_STATIC)
        set_source_files_properties(
            crc32_vx.c
            PROPERTIES COMPILE_OPTIONS "${VGFMAFLAG}")
    endif(HAS_S390X_VX_SUPPORT OR HAS_Z13_S390X_VX_SUPPORT)
endif(ZLIB_WITH_CRC32VX AND HAS_S390X_SUPPORT)
