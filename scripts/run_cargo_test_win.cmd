@echo off
setlocal

cd /d "%~dp0.."

rem VS Build Tools + Windows SDK are installed on this machine, but vcvars doesn't set SDK paths.
call C:\Progra~1\MIB055~1\2022\COMMUN~1\VC\Auxiliary\Build\vcvars64.bat

set CARGO_EXE=C:\Users\FATLIN~1\.cargo\bin\cargo.exe

set SDK_VER=10.0.19041.0
set SDK_ROOT=C:\Progra~2\WI3CF2~1\10
set VC_ROOT=C:\PROGRA~1\MIB055~1\2022\COMMUN~1\VC\Tools\MSVC\14.31.31103

set LIB=%VC_ROOT%\ATLMFC\lib\x64;%VC_ROOT%\lib\x64;%SDK_ROOT%\Lib\%SDK_VER%\um\x64;%SDK_ROOT%\Lib\%SDK_VER%\ucrt\x64
set INCLUDE=%VC_ROOT%\include;%VC_ROOT%\ATLMFC\include;%SDK_ROOT%\Include\%SDK_VER%\um;%SDK_ROOT%\Include\%SDK_VER%\shared;%SDK_ROOT%\Include\%SDK_VER%\ucrt;%SDK_ROOT%\Include\%SDK_VER%\winrt

echo [mjeku-desktop] Running cargo test...
%CARGO_EXE% test --manifest-path src-tauri\Cargo.toml

endlocal

