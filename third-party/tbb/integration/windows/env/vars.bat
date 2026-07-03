@echo off
REM
REM Copyright (c) 2005-2025 Intel Corporation
REM
REM Licensed under the Apache License, Version 2.0 (the "License");
REM you may not use this file except in compliance with the License.
REM You may obtain a copy of the License at
REM
REM     http://www.apache.org/licenses/LICENSE-2.0
REM
REM Unless required by applicable law or agreed to in writing, software
REM distributed under the License is distributed on an "AS IS" BASIS,
REM WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
REM See the License for the specific language governing permissions and
REM limitations under the License.
REM

REM Syntax:
REM  %SCRIPT_NAME% [^<arch^>] [^<vs^>]
REM    ^<arch^> should be one of the following
REM        ia32         : Set up for IA-32  architecture
REM        intel64      : Set up for Intel(R) 64  architecture
REM    if ^<arch^> is not set Intel(R) 64 architecture will be used
REM    ^<vs^> should be one of the following
REM        vs2019      : Set to use with Microsoft Visual Studio 2019 runtime DLLs
REM        vs2022      : Set to use with Microsoft Visual Studio 2022 runtime DLLs
REM        all         : Set to use oneTBB statically linked with Microsoft Visual C++ runtime
REM    if ^<vs^> is not set oneTBB dynamically linked with Microsoft Visual C++ runtime will be used.

set "SCRIPT_NAME=%~nx0"
set "TBB_SCRIPT_DIR=%~d0%~p0"
set "TBBROOT=%TBB_SCRIPT_DIR%.."

:: Set the default arguments
set TBB_TARGET_ARCH=intel64
set TBB_ARCH_SUFFIX=
set TBB_TARGET_VS=vc14

:ParseArgs
:: Parse the incoming arguments
if /i "%1"==""             goto ParseLayout
if /i "%1"=="ia32"         (set TBB_TARGET_ARCH=ia32)     & shift & goto ParseArgs
if /i "%1"=="intel64"      (set TBB_TARGET_ARCH=intel64)  & shift & goto ParseArgs
if /i "%1"=="vs2019"       (set TBB_TARGET_VS=vc14)       & shift & goto ParseArgs
if /i "%1"=="vs2022"       (set TBB_TARGET_VS=vc14)       & shift & goto ParseArgs
if /i "%1"=="all"          (set TBB_TARGET_VS=vc_mt)      & shift & goto ParseArgs

:ParseLayout
if exist "%TBBROOT%\redist\" (
    set "TBB_BIN_DIR=%TBBROOT%\redist"
    set "TBB_SUBDIR=%TBB_TARGET_ARCH%"
    goto SetEnv
)

if "%TBB_TARGET_ARCH%" == "ia32" (
    set TBB_ARCH_SUFFIX=32
)
if exist "%TBBROOT%\bin%TBB_ARCH_SUFFIX%" (
    set "TBB_BIN_DIR=%TBBROOT%\bin%TBB_ARCH_SUFFIX%"
    if "%TBB_TARGET_VS%" == "vc14" (
        set TBB_TARGET_VS=
    )
    goto SetEnv
)
:: Couldn't parse TBBROOT/bin, unset variable
set TBB_ARCH_SUFFIX=

if exist "%TBBROOT%\..\redist\" (
    set "TBB_BIN_DIR=%TBBROOT%\..\redist"
    set "TBB_SUBDIR=%TBB_TARGET_ARCH%\tbb"
    goto SetEnv
)

:SetEnv
if exist "%TBB_BIN_DIR%\%TBB_SUBDIR%\%TBB_TARGET_VS%\tbb12.dll" (
    set "TBB_DLL_PATH=%TBB_BIN_DIR%\%TBB_SUBDIR%\%TBB_TARGET_VS%"
) else (
    echo:
    echo :: ERROR: tbb12.dll library does not exist in "%TBB_BIN_DIR%\%TBB_SUBDIR%\%TBB_TARGET_VS%\"
    echo:
    exit /b 255
)

set "PATH=%TBB_DLL_PATH%;%PATH%"

set "LIB=%TBBROOT%\lib%TBB_ARCH_SUFFIX%\%TBB_SUBDIR%\%TBB_TARGET_VS%;%LIB%"
set "INCLUDE=%TBBROOT%\include;%INCLUDE%"
set "C_INCLUDE_PATH=%TBBROOT%\include;%C_INCLUDE_PATH%"
set "CPLUS_INCLUDE_PATH=%TBBROOT%\include;%CPLUS_INCLUDE_PATH%"
set "CMAKE_PREFIX_PATH=%TBBROOT%;%CMAKE_PREFIX_PATH%"
set "PKG_CONFIG_PATH=%TBBROOT%\lib%TBB_ARCH_SUFFIX%\pkgconfig;%PKG_CONFIG_PATH%"

:End
exit /B 0
