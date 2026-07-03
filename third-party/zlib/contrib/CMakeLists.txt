function(zlib_add_contrib_lib name description dir)
    option(ZLIB_BUILD_${name} "Enable building of ${description}" OFF)

    if(ZLIB_BUILD_${name})
        if(NOT DEFINED ZLIB_${name}_BUILD_SHARED)
            set(ZLIB_${name}_BUILD_SHARED ${ZLIB_BUILD_SHARED} CACHE BOOL "")
        endif(NOT DEFINED ZLIB_${name}_BUILD_SHARED)

        if(NOT DEFINED ZLIB_${name}_BUILD_STATIC)
            set(ZLIB_${name}_BUILD_STATIC ${ZLIB_BUILD_STATIC} CACHE BOOL "")
        endif(NOT DEFINED ZLIB_${name}_BUILD_STATIC)

        if(NOT DEFINED ZLIB_${name}_BUILD_TESTING)
            set(ZLIB_${name}_BUILD_TESTING ${ZLIB_BUILD_TESTING} CACHE BOOL "")
        endif(NOT DEFINED ZLIB_${name}_BUILD_TESTING)

        if(NOT DEFINED ZLIB_${name}_INSTALL)
            set(ZLIB_${name}_INSTALL ${ZLIB_INSTALL} CACHE BOOL "")
        endif(NOT DEFINED ZLIB_${name}_INSTALL)

        add_subdirectory(${dir}/)
    endif(ZLIB_BUILD_${name})
endfunction(zlib_add_contrib_lib name description dir)

function(zlib_add_contrib_feature name description dir)
    if(ARGC EQUAL 4)
        set(default_on ${ARGV3})
    else()
        set(default_on Off)
    endif()

    option(ZLIB_WITH_${name}
           "Enable build ${description}"
           ${default_on})

    if(ZLIB_WITH_${name})
        add_subdirectory(${dir}/)
    endif(ZLIB_WITH_${name})
endfunction(zlib_add_contrib_feature name description dir)

zlib_add_contrib_feature("GVMAT64"
    "of an optimized longest_match for 32 bits x86_64"
    gcc_gvmat64)

zlib_add_contrib_feature(INFBACK9 "with support for method 9 deflate" infback9)
zlib_add_contrib_feature(CRC32VX "with S390X-CRC32VX implementation" crc32vx)
zlib_add_contrib_lib(ADA "Ada bindings" ada)
zlib_add_contrib_lib(BLAST "blast binary" blast)
zlib_add_contrib_lib(IOSTREAM3 "IOStream C++ bindings V3" iostream3)
zlib_add_contrib_lib(MINIZIP "minizip library" minizip)
zlib_add_contrib_lib(PUFF "puff decompress library" puff)

if(WIN32)
    zlib_add_contrib_lib(TESTZLIB "testzlib binary" testzlib)

    if (ZLIB_BUILD_ZLIB1_DLL)
        add_subdirectory(zlib1-dll/)
    endif (ZLIB_BUILD_ZLIB1_DLL)

    option(ZLIB_BUILD_ZLIB1_DLL "Build the legacy zlib + minizip DLL" OFF)
endif(WIN32)
