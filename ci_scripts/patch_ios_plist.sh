#!/bin/bash
# Përgatit Info.plist(-et) e projektit Xcode të gjeneruar nga `tauri ios init`:
# leje Camera/Photo Library (Foto Para/Pas) + File Sharing (Files app → "On My iPhone")
# për PDF-të e eksportuara (fatura/receta). Përdoret nga .github/workflows/ios-init.yml
# dhe .github/workflows/ios-build.yml që të mos përsëritet logjika.
set -e

APPLE_DIR="${1:-src-tauri/gen/apple}"

found=0
while IFS= read -r plist; do
  found=1
  echo "Patching $plist"
  /usr/libexec/PlistBuddy -c "Set :NSCameraUsageDescription Mjeku përdor kamerën për të shtuar foto Para/Pas te kartela e pacientit." "$plist" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :NSCameraUsageDescription string Mjeku përdor kamerën për të shtuar foto Para/Pas te kartela e pacientit." "$plist"
  /usr/libexec/PlistBuddy -c "Set :NSPhotoLibraryUsageDescription Mjeku përdor galerinë e fotove për të bashkangjitur foto te kartela e pacientit." "$plist" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :NSPhotoLibraryUsageDescription string Mjeku përdor galerinë e fotove për të bashkangjitur foto te kartela e pacientit." "$plist"
  # Bën të dukshëm folderin Documents të app-it te Files → "On My iPhone" — pa
  # këto dy çelësa, PDF-të e eksportuara (fatura/receta) mbeten të fshehura brenda
  # sandbox-it dhe përdoruesi s'ka asnjë mënyrë t'i hapë/printojë nga iOS.
  /usr/libexec/PlistBuddy -c "Set :UIFileSharingEnabled true" "$plist" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :UIFileSharingEnabled bool true" "$plist"
  /usr/libexec/PlistBuddy -c "Set :LSSupportsOpeningDocumentsInPlace true" "$plist" 2>/dev/null || \
    /usr/libexec/PlistBuddy -c "Add :LSSupportsOpeningDocumentsInPlace bool true" "$plist"
done < <(find "$APPLE_DIR" -name "Info.plist" -not -path "*/Pods/*")

if [ "$found" -eq 0 ]; then
  echo "::warning::Asnjë Info.plist nuk u gjet nën $APPLE_DIR — kontrollo strukturën e projektit."
fi
