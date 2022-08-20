# Create a symbolic link with a relative path. GNU ln takes the --relative
# option to do that, but that's not portable, so we do it in CMake.

cmake_minimum_required(VERSION 3.14)
get_filename_component(DIR "${DEST}" DIRECTORY)
file(RELATIVE_PATH PATH "/${DIR}" "/${SOURCE}")
file(CREATE_LINK "${PATH}" "${DEST}" SYMBOLIC)
