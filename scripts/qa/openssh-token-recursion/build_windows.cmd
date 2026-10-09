@echo off
setlocal
set "SRC=%~dp0test_pRtlNtStatusToDosError_recursion.c"
set "OUT_DIR=%~dp0bin"
if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"
if errorlevel 1 exit /b %ERRORLEVEL%
gcc -Wall -Wextra -Werror -O2 "%SRC%" -o "%OUT_DIR%\test_prefix.exe"
if errorlevel 1 exit /b %ERRORLEVEL%
gcc -Wall -Wextra -Werror -O2 -DTEST_FIXED_VERSION "%SRC%" -o "%OUT_DIR%\test_postfix.exe"
if errorlevel 1 exit /b %ERRORLEVEL%
echo BUILD_OK
