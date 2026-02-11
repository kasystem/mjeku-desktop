@echo off
setlocal

cd /d "%~dp0.."

rem VS Build Tools + Windows SDK are installed on this machine, but vcvars doesn't set SDK paths.
call C:\Progra~1\MIB055~1\2022\COMMUN~1\VC\Auxiliary\Build\vcvars64.bat

set CARGO_EXE=C:\Users\FATLIN~1\.cargo\bin\cargo.exe
rem Ensure cargo is discoverable (tauri CLI calls `cargo metadata`).
set PATH=C:\Users\FATLIN~1\.cargo\bin;%PATH%

set SDK_VER=10.0.19041.0
set SDK_ROOT=C:\Progra~2\WI3CF2~1\10
set VC_ROOT=C:\PROGRA~1\MIB055~1\2022\COMMUN~1\VC\Tools\MSVC\14.31.31103

set LIB=%VC_ROOT%\ATLMFC\lib\x64;%VC_ROOT%\lib\x64;%SDK_ROOT%\Lib\%SDK_VER%\um\x64;%SDK_ROOT%\Lib\%SDK_VER%\ucrt\x64
set INCLUDE=%VC_ROOT%\include;%VC_ROOT%\ATLMFC\include;%SDK_ROOT%\Include\%SDK_VER%\um;%SDK_ROOT%\Include\%SDK_VER%\shared;%SDK_ROOT%\Include\%SDK_VER%\ucrt;%SDK_ROOT%\Include\%SDK_VER%\winrt

set NODE_DIR=C:\Users\FATLIN~1\Desktop\Mjeku\.tools\node-v20.20.0-win-x64
set NODE20=%NODE_DIR%\node.exe

rem Ensure `node` and `npm` are available for Tauri `beforeBuildCommand`.
set PATH=%NODE_DIR%;%PATH%

echo [mjeku-desktop] Starting tauri build (release)...
echo [mjeku-desktop] Using cargo:
%CARGO_EXE% -V

rem This will run src-tauri/tauri.conf.json beforeBuildCommand (build UI + seed zip).
"%NODE20%" node_modules\@tauri-apps\cli\tauri.js build --verbose --runner %CARGO_EXE%

endlocal

