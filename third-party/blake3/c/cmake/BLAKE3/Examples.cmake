if(NOT WIN32)
  add_executable(blake3-example
    example.c)
  target_link_libraries(blake3-example PRIVATE blake3)
  install(TARGETS blake3-example)

  if(BLAKE3_USE_TBB)
    add_executable(blake3-example-tbb
      example_tbb.c)
    target_link_libraries(blake3-example-tbb PRIVATE blake3)
    install(TARGETS blake3-example-tbb)
  endif()
endif()
