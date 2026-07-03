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

set(REACHED_LIBS FALSE)
set(REACHED_OBJ FALSE)
foreach(arg RANGE 5 ${CMAKE_ARGC})
    if(CMAKE_ARGV${arg} STREQUAL LIBS)
        set(REACHED_LIBS TRUE)
        set(REACHED_OBJ FALSE)
        continue()
    endif(CMAKE_ARGV${arg} STREQUAL LIBS)

    if(CMAKE_ARGV${arg} STREQUAL OBJ)
        set(REACHED_LIBS FALSE)
        set(REACHED_OBJ TRUE)
        continue()
    endif(CMAKE_ARGV${arg} STREQUAL OBJ)

    if(CMAKE_ARGC EQUAL arg)
        continue()
    endif(CMAKE_ARGC EQUAL arg)

    if(REACHED_LIBS)
        list(APPEND LIBS "${CMAKE_ARGV${arg}}")
    elseif(REACHED_OBJ AND NOT ALI)
        string(REPLACE ".o" ".ali" ALI "${CMAKE_ARGV${arg}}")
    else(REACHED_LIBS)
        string(SUBSTRING "${CMAKE_ARGV${arg}}" 0 3 start)

        if(NOT start STREQUAL -aO)
            list(APPEND FLAGS "${CMAKE_ARGV${arg}}")
        endif(NOT start STREQUAL -aO)
    endif(REACHED_LIBS)
endforeach(arg RANGE 5 ${CMAKE_ARGC})

execute_process(COMMAND ${CMAKE_ARGV3} link ${ALI} -o ${CMAKE_ARGV4} ${FLAGS} ${OTHER_OBJECTS} ${LIBS}
                RESULT_VARIABLE RESULT
                OUTPUT_VARIABLE dont_care
                ERROR_VARIABLE ERROR)

if(RESULT)
    message(FATAL_ERROR ${RESULT} ${ERROR})
endif(RESULT)
