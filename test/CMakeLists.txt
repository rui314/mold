function(mold_add_arch_test NAME DIR TRIPLE MACHINE)
    set(TEST_NAME "${NAME}_arch_${MACHINE}")
    add_test(NAME ${TEST_NAME}
        COMMAND ${DIR}/${NAME}.sh
        WORKING_DIRECTORY ${mold_BINARY_DIR})

    set(TEST_ENV
        TEST_CC=${TRIPLE}-gcc
        TEST_CXX=${TRIPLE}-g++
        TEST_GCC=${TRIPLE}-gcc
        TEST_GXX=${TRIPLE}-g++
        OBJDUMP=${TRIPLE}-objdump
        MACHINE=${MACHINE}
        "QEMU=qemu-${MACHINE} -L /usr/${TRIPLE}")
    set_tests_properties(${TEST_NAME} PROPERTIES ENVIRONMENT "${TEST_ENV}")
endfunction()

function(mold_add_arch_tests NAME DIR)
    mold_add_arch_test(${NAME} ${DIR} x86_64-linux-gnu x86_64)
    mold_add_arch_test(${NAME} ${DIR} i686-linux-gnu i386)
    mold_add_arch_test(${NAME} ${DIR} aarch64-linux-gnu aarch64)
    mold_add_arch_test(${NAME} ${DIR} arm-linux-gnueabihf arm)
    mold_add_arch_test(${NAME} ${DIR} riscv64-linux-gnu riscv64)
endfunction()

if(CMAKE_SYSTEM_NAME MATCHES Linux)
    add_subdirectory(elf)
elseif(CMAKE_SYSTEM_NAME MATCHES Darwin)
    add_subdirectory(macho)
endif()
