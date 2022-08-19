option(MOLD_ENABLE_QEMU_TESTS "Enable tests for other targets" OFF)

file(GLOB MOLD_ELF_TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} "*.sh")

function(mold_add_arch_test NAME TRIPLE MACHINE)
  if(NOT ${CMAKE_HOST_SYSTEM_PROCESSOR} STREQUAL ${MACHINE})
    set(TEST_NAME "${MACHINE}-${NAME}")
    add_test(NAME ${TEST_NAME}
      COMMAND bash -x ${CMAKE_CURRENT_LIST_DIR}/${NAME}
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
  endif()
endfunction()

foreach(TEST IN LISTS MOLD_ELF_TESTS)
  add_test(NAME "${CMAKE_HOST_SYSTEM_PROCESSOR}-${TEST}"
    COMMAND bash -x ${CMAKE_CURRENT_LIST_DIR}/${TEST}
    WORKING_DIRECTORY ${mold_BINARY_DIR})

  if(MOLD_ENABLE_QEMU_TESTS)
    mold_add_arch_test(${TEST} x86_64-linux-gnu x86_64)
    mold_add_arch_test(${TEST} i686-linux-gnu i386)
    mold_add_arch_test(${TEST} aarch64-linux-gnu aarch64)
    mold_add_arch_test(${TEST} arm-linux-gnueabihf arm)
    mold_add_arch_test(${TEST} riscv64-linux-gnu riscv64)
  endif()
endforeach()
