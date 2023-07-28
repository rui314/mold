# Installation from Sources


## Prerequisites 
   
   - Make sure you have installed CMake version 3.1 (or newer) on your system. oneTBB uses CMake build configuration.
   - Configure and build oneTBB. To work with build configurations, see [Build System Description](cmake/README.md). 


## Configure oneTBB

At the command prompt, type:
```
cmake <options> <repo_root>
```

You may want to use some additional options for configuration:

| Option                    | Purpose                   | Description                                                                        |
| ------                    |------                     | ------                                                                             |
| `-G <generator>`          | Specify project generator | For more information, run cmake `–help`.                                           |
|`-DCMAKE_BUILD_TYPE=Debug` | Specify for Debug build   | Not applicable for multi-configuration generators such as Visual Studio generator. |


## Build oneTBB
 
To build the system, run:
```
cmake --build . <options>
```

Some useful build options:
- `--target <target>` - specific target, "all" is default.
-	`--config <Release|Debug>` - build configuration, applicable only for multi-config generators such as Visual Studio generator.


## Install and Pack oneTBB

---
**NOTE**

Be careful about installing prefix. It defaults to `/usr/local` on UNIX* and `c:/Program Files/${PROJECT_NAME}` on Windows* OS.
You can define custom `CMAKE_INSTALL_PREFIX` during configuration:

```
cmake -DCMAKE_INSTALL_PREFIX=/my/install/prefix ..
```

---

Installation can also be done using:

```
cmake --install <project-binary-dir>
```

Special ``--install`` target can alternatively be used for installation, e.g. ``make install``.

You can use the ``install`` components for partial installation.

The following install components are supported:
- `runtime` - oneTBB runtime package (core shared libraries and `.dll` files on Windows* OS).
- `devel` - oneTBB development package (header files, CMake integration files, library symbolic links, and `.lib` files on Windows* OS).
- `tbb4py` - [oneTBB Module for Python](#onetbb-python-module-support).

If you want to install specific components after configuration and build, run:

```bash
cmake -DCOMPONENT=<component> [-DBUILD_TYPE=<build-type>] -P cmake_install.cmake
```

Simple packaging using CPack is supported.
The following commands allow you to create a simple portable package that includes header files, libraries, and integration files for CMake:

```bash
cmake <options> ..
cpack
```

## Installation from vcpkg

You can download and install oneTBB using the [vcpkg](https://github.com/Microsoft/vcpkg) dependency manager:
```sh
  git clone https://github.com/Microsoft/vcpkg.git
  cd vcpkg
  ./bootstrap-vcpkg.sh #.\bootstrap-vcpkg.bat(for Windows)
  ./vcpkg integrate install
  ./vcpkg install tbb
```

The oneTBB port in vcpkg is kept up to date by Microsoft* team members and community contributors. If the version is out of date, create an issue or pull request on the [vcpkg repository](https://github.com/Microsoft/vcpkg).

## Example of Installation

### Single-configuration generators

The following example demonstrates how to install oneTBB for single-configuration generators (e.g. GNU Make, Ninja, etc.).
```bash
# Do our experiments in /tmp
cd /tmp
# Clone oneTBB repository
git clone https://github.com/oneapi-src/oneTBB.git
cd oneTBB
# Create binary directory for out-of-source build
mkdir build && cd build
# Configure: customize CMAKE_INSTALL_PREFIX and disable TBB_TEST to avoid tests build
cmake -DCMAKE_INSTALL_PREFIX=/tmp/my_installed_onetbb -DTBB_TEST=OFF ..
# Build
cmake --build .
# Install
cmake --install .
# Well done! Your installed oneTBB is in /tmp/my_installed_onetbb
```

### Multi-configuration generators

The following example demonstrates how to install oneTBB for multi-configuration generators such as Visual Studio*. 

Choose the configuration during the build and install steps:
```batch
REM Do our experiments in %TMP%
cd %TMP%
REM Clone oneTBB repository
git clone https://github.com/oneapi-src/oneTBB.git
cd oneTBB
REM Create binary directory for out-of-source build
mkdir build && cd build
REM Configure: customize CMAKE_INSTALL_PREFIX and disable TBB_TEST to avoid tests build
cmake -DCMAKE_INSTALL_PREFIX=%TMP%\my_installed_onetbb -DTBB_TEST=OFF ..
REM Build "release with debug information" configuration 
cmake --build . --config relwithdebinfo
REM Install "release with debug information" configuration 
cmake --install . --config relwithdebinfo
REM Well done! Your installed oneTBB is in %TMP%\my_installed_onetbb
```
