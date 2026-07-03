#CMAKE_ARGV0 = /path/to/cmake
#CMAKE_ARGV1 = -P
#CMAKE_ARGV2 = path/to/this/file
#CMAKE_ARGV3 = linker
#CMAKE_ARGV4 = output-name
#CMAKE_ARGV5...CMAKE_AGVN = OBJECTS
#CMAKE_ARGVN+1 = LIBS
#CMAKE_ARGVN+2...CMAKE_ARGVM libraries

if(NOT CMAKE_ARGV3)
    message(FATAL_ERROR "linker not set")
endif(NOT CMAKE_ARGV3)

set(REACHED_FILES FALSE)
foreach(arg RANGE 5 ${CMAKE_ARGC})
    if(CMAKE_ARGV${arg} STREQUAL "LIBS")
        set(REACHED_FILES TRUE)
        continue()
    endif(CMAKE_ARGV${arg} STREQUAL "LIBS")

    if(CMAKE_ARGC EQUAL arg)
        continue()
    endif(CMAKE_ARGC EQUAL arg)

    if(REACHED_LIBS)
        list(APPEND LIBS "${CMAKE_ARGV${arg}} ")
    else(REACHED_LIBS)
        list(APPEND OBJECT_FILES "${CMAKE_ARGV${arg}}")
    endif(REACHED_LIBS)
endforeach(arg RANGE 5 ${CMAKE_ARGC})

file(WRITE dummylib.adb
    "procedure dummylib is\n"
    "begin\n"
    "   null;\n"
    "end;\n")

execute_process(COMMAND ${CMAKE_ARGV3} compile -fPIC dummylib.adb
                OUTPUT_VARIABLE dont_care
                ERROR_VARIABLE ERROR)
execute_process(COMMAND ${CMAKE_ARGV3} bind -n dummylib.ali
                OUTPUT_VARIABLE dont_care
                ERROR_VARIABLE ERROR)

execute_process(COMMAND ${CMAKE_ARGV3} link -shared dummylib.ali -o ${CMAKE_ARGV4} ${OBJECT_FILES} ${LIBS}
                RESULT_VARIABLE RESULT
                OUTPUT_VARIABLE dont_care
                ERROR_VARIABLE ERROR)

if(RESULT)
    message(FATAL_ERROR ${RESULT} ${ERROR})
endif(RESULT)
