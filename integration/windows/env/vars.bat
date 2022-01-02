@echo off
REM
REM Copyright (c) 2005-2021 Intel Corporation
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
REM        vs2015      : Set to use with Microsoft Visual Studio 2015 runtime DLLs
REM        vs2017      : Set to use with Microsoft Visual Studio 2017 runtime DLLs
REM        vs2019      : Set to use with Microsoft Visual Studio 2019 runtime DLLs
REM        all         : Set to use TBB statically linked with Microsoft Visual C++ runtime
REM    if ^<vs^> is not set TBB statically linked with Microsoft Visual C++ runtime will be used.

set "SCRIPT_NAME=%~nx0"
set "TBB_BIN_DIR=%~d0%~p0"
set "TBBROOT=%TBB_BIN_DIR%.."

:: Set the default arguments
set TBB_TARGET_ARCH=intel64
set TBB_TARGET_VS=vc_mt

:ParseArgs
:: Parse the incoming arguments
if /i "%1"==""        goto SetEnv
if /i "%1"=="ia32"         (set TBB_TARGET_ARCH=ia32)     & shift & goto ParseArgs
if /i "%1"=="intel64"      (set TBB_TARGET_ARCH=intel64)  & shift & goto ParseArgs
if /i "%1"=="vs2015"       (set TBB_TARGET_VS=vc14)       & shift & goto ParseArgs
if /i "%1"=="vs2017"       (set TBB_TARGET_VS=vc14)       & shift & goto ParseArgs
if /i "%1"=="vs2019"       (set TBB_TARGET_VS=vc14)       & shift & goto ParseArgs
if /i "%1"=="all"          (set TBB_TARGET_VS=vc_mt)      & shift & goto ParseArgs

:SetEnv
if exist "%TBBROOT%\redist\%TBB_TARGET_ARCH%\%TBB_TARGET_VS%\tbb12.dll" (
    set "TBB_DLL_PATH=%TBBROOT%\redist\%TBB_TARGET_ARCH%\%TBB_TARGET_VS%"
)
if exist "%TBBROOT%\..\redist\%TBB_TARGET_ARCH%\tbb\%TBB_TARGET_VS%\tbb12.dll" (
    set "TBB_DLL_PATH=%TBBROOT%\..\redist\%TBB_TARGET_ARCH%\tbb\%TBB_TARGET_VS%"
)

set "PATH=%TBB_DLL_PATH%;%PATH%"

set "LIB=%TBBROOT%\lib\%TBB_TARGET_ARCH%\%TBB_TARGET_VS%;%LIB%"
set "INCLUDE=%TBBROOT%\include;%INCLUDE%"
set "CPATH=%TBBROOT%\include;%CPATH%"
set "CMAKE_PREFIX_PATH=%TBBROOT%;%CMAKE_PREFIX_PATH%"
set "PKG_CONFIG_PATH=%TBBROOT%\lib\pkgconfig;%PKG_CONFIG_PATH%"

:End
exit /B 0
