@echo off
rem ------------------------------------------------------------------
rem Build a binary release
rem ------------------------------------------------------------------
setlocal

mkdir "out/bundle"

set MI_TAG=
for /F "tokens=*" %%x in ('git describe --tag 2^> nul') do (set MI_TAG=%%x)
set MI_COMMIT=
for /F "tokens=*" %%x in ('git rev-parse HEAD 2^> nul') do (set MI_COMMIT=%%x)

echo.
echo Tag   : %MI_TAG%
echo Commit: %MI_COMMIT%

rem (not needed as it is already done on other platforms)
rem curl --proto =https --tlsv1.2 -f -L -o "https://github.com/microsoft/mimalloc/archive/%MI_COMMIT%.tar.gz" "out/bundle/mimalloc-%MI_TAG%-source.tar.gz"

call:bundle x64 debug   Debug   "-A x64 -T ClangCl"  || goto :err
call:bundle x64 release Release "-A x64 -T ClangCl -DMI_OPT_ARCH=ON"  || goto :err
call:bundle x64 secure  Release "-A x64 -T ClangCl -DMI_OPT_ARCH=ON -DMI_SECURE=ON"  || goto:err
call:archive x64 || goto:err

call:bundle x86 debug   Debug   "-A Win32 -T ClangCl"  || goto :err
call:bundle x86 release Release "-A Win32 -T ClangCl -DMI_OPT_ARCH=ON"  || goto :err
call:bundle x86 secure  Release "-A Win32 -T ClangCl -DMI_OPT_ARCH=ON -DMI_SECURE=ON"  || goto:err
call:archive x86 || goto:err

call:bundle arm64 debug   Debug   "-A ARM64 -T ClangCl"  || goto :err
call:bundle arm64 release Release "-A ARM64 -T ClangCl -DMI_OPT_ARCH=ON"  || goto :err
call:bundle arm64 secure  Release "-A ARM64 -T ClangCl -DMI_OPT_ARCH=ON -DMI_SECURE=ON"  || goto:err
call:archive arm64 || goto:err

call:bundle arm64ec debug   Debug   "-A Arm64EC"  || goto :err
call:bundle arm64ec release Release "-A Arm64EC -DMI_OPT_ARCH=ON"  || goto :err
call:bundle arm64ec secure  Release "-A Arm64EC -DMI_OPT_ARCH=ON -DMI_SECURE=ON"  || goto:err
call:archive arm64ec || goto:err

echo.
echo Done
exit /b 0

:err
echo Failed with error: %errorlevel%
exit /b %errorlevel%


:bundle
rem bundle: 1:arch 2:build-variant 3:build-type 4:cmake options
echo.
echo Build: %~1 %~2: %~3
mkdir "out/bundle/prefix-%~1"
cmake . -B "out/bundle/%~2-%~1" -DCMAKE_BUILD_TYPE=%~3 %~4 || goto :err
cmake --build "out/bundle/%~2-%~1" --config %~3 || goto :err
if "%~1" == "x64" (ctest --test-dir "out/bundle/%~2-%~1" -C %~3 || goto :err)
if "%~1" == "x86" (ctest --test-dir "out/bundle/%~2-%~1" -C %~3 || goto :err)
cmake --install "out/bundle/%~2-%~1" --prefix "out/bundle/prefix-%~1" --config %~3 || goto :err
exit /b %errorlevel%


:archive
rem archive: 1:arch
echo.
echo Archive: %~1
if "%~1" == "x64"     (xcopy /Y /F "bin/minject.exe" "out/bundle/prefix-%~1/bin")
if "%~1" == "x86"     (xcopy /Y /F "bin/minject32.exe" "out/bundle/prefix-%~1/bin")
if "%~1" == "arm64"   (xcopy /Y /F "bin/minject-arm64.exe" "out/bundle/prefix-%~1/bin")
if "%~1" == "arm64ec" (xcopy /Y /F "bin/minject-arm64.exe" "out/bundle/prefix-%~1/bin")
cd "out/bundle/prefix-%~1"
tar -czvf "../mimalloc-%MI_TAG%-windows-%~1.tar.gz" .
cd ../../..
exit /b %errorlevel%
