option(MOLD_ENABLE_QEMU_TESTS "Enable tests on non-native targets" OFF)
option(MOLD_ENABLE_QEMU_TESTS_RV32 "Enable tests for RV32" OFF)
option(MOLD_ENABLE_QEMU_TESTS_SPARC64 "Enable tests for SPARC64" OFF)

function(add_target ARCH TRIPLE)
  if(MOLD_ENABLE_QEMU_TESTS OR ${CMAKE_HOST_SYSTEM_PROCESSOR} STREQUAL ${ARCH})
    file(GLOB ALL_TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} CONFIGURE_DEPENDS
      "*.sh")

    list(FILTER ALL_TESTS EXCLUDE REGEX "_")

    file(GLOB TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} CONFIGURE_DEPENDS
      "${ARCH}_*.sh")

    list(APPEND TESTS ${ALL_TESTS})

    set(ENV
      TEST_CC=${TRIPLE}-gcc
      TEST_CXX=${TRIPLE}-g++
      TEST_GCC=${TRIPLE}-gcc
      TEST_GXX=${TRIPLE}-g++
      TEST_TRIPLE=${TRIPLE}-
      MACHINE=${ARCH}
      "QEMU=qemu-${ARCH} -L /usr/${TRIPLE}")

    foreach(TEST IN LISTS TESTS)
      add_test(NAME "${ARCH}-${TEST}"
        COMMAND bash -x ${CMAKE_CURRENT_LIST_DIR}/${TEST}
        WORKING_DIRECTORY ${mold_BINARY_DIR})

      if(NOT ${CMAKE_HOST_SYSTEM_PROCESSOR} STREQUAL ${ARCH})
        set_tests_properties("${ARCH}-${TEST}" PROPERTIES
          ENVIRONMENT "${ENV}")
      endif()
    endforeach()
  endif()
endfunction()

add_target(x86_64 x86_64-linux-gnu)
add_target(i386 i686-linux-gnu)
add_target(aarch64 aarch64-linux-gnu)
add_target(arm arm-linux-gnueabihf)
add_target(riscv64 riscv64-linux-gnu)
add_target(ppc64le powerpc64le-linux-gnu)

if(MOLD_ENABLE_QEMU_TESTS_RV32)
  add_target(riscv32 riscv32-linux-gnu)
endif()

if(MOLD_ENABLE_QEMU_TESTS_SPARC64)
  add_target(sparc64 sparc64-linux-gnu)
endif()
