include(CMakeLanguageInformation)

set(CMAKE_ADA_OUTPUT_EXTENSION .o)
set(CMAKE_ADA_OUTPUT_EXTENSION_REPLACE TRUE)

if(CMAKE_USER_MAKE_RULES_OVERRIDE)
    include(${CMAKE_USER_MAKE_RULES_OVERRIDE} RESULT_VARIABLE _override)
    set(CMAKE_USER_MAKE_RULES_OVERRIDE "${_override}")
endif(CMAKE_USER_MAKE_RULES_OVERRIDE)


if(CMAKE_USER_MAKE_RULES_OVERRIDE_ADA)
    include(${CMAKE_USER_MAKE_RULES_OVERRIDE_ADA} RESULT_VARIABLE _override)
    set(CMAKE_USER_MAKE_RULES_OVERRIDE_ADA "${_override}")
endif(CMAKE_USER_MAKE_RULES_OVERRIDE_ADA)

set(CMAKE_ADA_FLAGS_INIT "$ENV{ADAFLAGS} ${CMAKE_ADA_FLAGS_INIT}")

string(APPEND CMAKE_ADA_FLAGS_INIT " ")
string(APPEND CMAKE_ADA_FLAGS_DEBUG_INIT " -g")
string(APPEND CMAKE_ADA_FLAGS_MINSIZEREL_INIT " -Os")
string(APPEND CMAKE_ADA_FLAGS_RELEASE_INIT " -O3")
string(APPEND CMAKE_ADA_FLAGS_RELWITHDEBINFO_INIT " -O2 -g")

cmake_initialize_per_config_variable(CMAKE_ADA_FLAGS "Flags used by the Ada compiler")

if(CMAKE_ADA_STANDARD_LIBRARIES_INIT)
    set(CMAKE_ADA_STANDARD_LIBRARIES
        "${CMAKE_ADA_STANDARD_LIBRARIES_INIT}"
        CACHE
        STRING "Libraries linked by default with all Ada applications.")
    mark_as_advanced(CMAKE_ADA_STANDARD_LIBRARIES)
endif(CMAKE_ADA_STANDARD_LIBRARIES_INIT)

if(NOT CMAKE_ADA_COMPILER_LAUNCHER AND DEFINED ENV{CMAKE_ADA_COMPILER_LAUNCHER})
    set(CMAKE_ADA_COMPILER_LAUNCHER
        "$ENV{CMAKE_ADA_COMPILER_LAUNCHER}"
        CACHE
        STRING "Compiler launcher for Ada.")
endif(NOT CMAKE_ADA_COMPILER_LAUNCHER AND DEFINED ENV{CMAKE_ADA_COMPILER_LAUNCHER})

if(NOT CMAKE_ADA_LINKER_LAUNCHER AND DEFINED ENV{CMAKE_ADA_LINKER_LAUNCHER})
    set(CMAKE_ADA_LINKER_LAUNCHER
        "$ENV{CMAKE_ADA_LINKER_LAUNCHER}"
        CACHE
        STRING "Linker launcher for Ada.")
endif(NOT CMAKE_ADA_LINKER_LAUNCHER AND DEFINED ENV{CMAKE_ADA_LINKER_LAUNCHER})

include(CMakeCommonLanguageInclude)
_cmake_common_language_platform_flags(ADA)

if(NOT CMAKE_ADA_CREATE_SHARED_LIBRARY)
    set(CMAKE_ADA_CREATE_SHARED_LIBRARY
        "${CMAKE_ADA_BINDER_HELPER} <CMAKE_ADA_COMPILER> <OBJECTS> FLAGS <FLAGS> <LINK_FLAGS>"
        "${CMAKE_ADA_SHARED_LINK_HELPER} <CMAKE_ADA_COMPILER> <TARGET> <OBJECTS> <LINK_LIBRARIES>")
endif(NOT CMAKE_ADA_CREATE_SHARED_LIBRARY)

if(NOT CMAKE_ADA_CREATE_STATIC_LIBRARY)
    set(CMAKE_ADA_CREATE_STATIC_LIBRARY
        "${CMAKE_ADA_STATIC_LINK_HELPER} ${CMAKE_AR} <TARGET> <OBJECTS>")
endif(NOT CMAKE_ADA_CREATE_STATIC_LIBRARY)

if(NOT CMAKE_ADA_COMPILE_OBJECT)
    set(CMAKE_ADA_COMPILE_OBJECT
        "${CMAKE_ADA_COMPILER_HELPER} <CMAKE_ADA_COMPILER> <OBJECT_DIR> <SOURCE> <FLAGS>")
endif(NOT CMAKE_ADA_COMPILE_OBJECT)

if(NOT CMAKE_ADA_LINK_EXECUTABLE)
    set(CMAKE_ADA_LINK_EXECUTABLE
        "${CMAKE_ADA_BINDER_HELPER} <CMAKE_ADA_COMPILER> <OBJECTS> FLAGS <FLAGS> <LINK_FLAGS>"
        "${CMAKE_ADA_EXE_LINK_HELPER} <CMAKE_ADA_COMPILER> <TARGET> <FLAGS> <CMAKE_C_LINK_FLAGS> <LINK_FLAGS> OBJ <OBJECTS> LIBS <LINK_LIBRARIES>")
endif(NOT CMAKE_ADA_LINK_EXECUTABLE)

function(ada_add_executable)
    if(ARGC GREATER 1)
        math(EXPR last_index "${ARGC} - 1")
        foreach(source RANGE 1 ${last_index})
            list(APPEND SOURCES ${ARGV${source}})
            string(REPLACE ".adb" "" ali "${ARGV${source}}")
            set(clean_file "CMakeFiles/${ARGV0}.dir/${ali}.ali")
            list(APPEND CLEAN_FILES ${clean_file})
            list(APPEND CLEAN_FILES b~${ali}.adb)
            list(APPEND CLEAN_FILES b~${ali}.ads)
            list(APPEND CLEAN_FILES b~${ali}.ali)
            list(APPEND CLEAN_FILES b~${ali}.o)
        endforeach(source RANGE 1 ${ARGC})

        add_executable(${ARGV0} ${ARGV1} ${SOURCES})

        set_target_properties(${ARGV0}
            PROPERTIES
                ADDITIONAL_CLEAN_FILES "${CLEAN_FILES}")
    endif(ARGC GREATER 1)
endfunction(ada_add_executable)

function(ada_add_library)
    if(ARGC GREATER 2)
        math(EXPR last_index "${ARGC} - 1")
        foreach(source RANGE 2 ${last_index})
            list(APPEND SOURCES ${ARGV${source}})
            string(REPLACE ".adb" "" ali "${ARGV${source}}")
            set(clean_file "CMakeFiles/${ARGV0}.dir/${ali}.ali")
            list(APPEND CLEAN_FILES ${clean_file})
            list(APPEND CLEAN_FILES b~${ali}.adb)
            list(APPEND CLEAN_FILES b~${ali}.ads)
            list(APPEND CLEAN_FILES b~${ali}.ali)
            list(APPEND CLEAN_FILES b~${ali}.o)
        endforeach(source RANGE 2 ${ARGC})

        add_library(${ARGV0} ${ARGV1} ${SOURCES})

        set_target_properties(${ARGV0}
            PROPERTIES
                ADDITIONAL_CLEAN_FILES "${CLEAN_FILES};dummylib.adb;dummylib.ali;dummylib.o"
                ALI_FLAG "-aO${CMAKE_CURRENT_BINARY_DIR}/CMakeFiles/${ARGV0}.dir/")
    endif(ARGC GREATER 2)
endfunction(ada_add_library)

function(ada_find_ali)
    get_target_property(link_libs ${ARGV0} LINK_LIBRARIES)

    foreach(lib IN LISTS link_libs)
        get_target_property(ali ${lib} ALI_FLAG)
        string(APPEND FLAGS ${ali} " ")
        unset(ali)
    endforeach(lib IN LISTS link_libs)

    set_target_properties(${ARGV0}
        PROPERTIES
            LINK_FLAGS ${FLAGS})
endfunction(ada_find_ali)

set(CMAKE_ADA_INFORMATION_LOADED TRUE)
