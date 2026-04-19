@echo off
REM
REM Copyright (c) 2023 Intel Corporation
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

if not defined SETVARS_CALL (
    echo:
    echo :: ERROR: This script must be executed by setvars.bat.
    echo:   Try '[install-dir]\setvars.bat --help' for help.
    echo:
    exit /b 255
)

if not defined ONEAPI_ROOT (
    echo:
    echo :: ERROR: This script requires that the ONEAPI_ROOT env variable is set."
    echo:   Try '[install-dir]\setvars.bat --help' for help.
    echo:
    exit /b 254
)

set "TBBROOT=%ONEAPI_ROOT%"

:: Set the default arguments
set "TBB_TARGET_ARCH=%INTEL_TARGET_ARCH%"
set TBB_TARGET_VS=
set ARCH_SUFFIX=

:ParseArgs
:: Parse the incoming arguments
if /i "%1"==""        goto SetEnv
if /i "%1"=="vs2019"       (set TBB_TARGET_VS= )       & shift & goto ParseArgs
if /i "%1"=="vs2022"       (set TBB_TARGET_VS= )       & shift & goto ParseArgs
if /i "%1"=="all"          (set TBB_TARGET_VS=vc_mt)   & shift & goto ParseArgs

if "%TBB_TARGET_ARCH%"=="ia32" set ARCH_SUFFIX=32  

:SetEnv
if exist "%TBBROOT%\bin%ARCH_SUFFIX%\%TBB_TARGET_VS%\tbb12.dll" (
    set "TBB_DLL_PATH=%TBBROOT%\bin%ARCH_SUFFIX%\%TBB_TARGET_VS%"
)

:End
exit /B 0
