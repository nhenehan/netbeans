; Licensed to the Apache Software Foundation (ASF) under one
; or more contributor license agreements.  See the NOTICE file
; distributed with this work for additional information
; regarding copyright ownership.  The ASF licenses this file
; to you under the Apache License, Version 2.0 (the
; "License"); you may not use this file except in compliance
; with the License.  You may obtain a copy of the License at
;
;   http://www.apache.org/licenses/LICENSE-2.0
;
; Unless required by applicable law or agreed to in writing,
; software distributed under the License is distributed on an
; "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
; KIND, either express or implied.  See the License for the
; specific language governing permissions and limitations
; under the License.

; NSIS installer for the cnd + java + web + rust Windows release.
;
; makensis -DSRC=<unzipped>/netbeans -DVERSION=<x.y.z> -DOUTFILE=<installer.exe> netbeans-installer.nsi
;
; SRC     the netbeans/ directory from the windows-x64 release zip (required);
;         SRC\..\size-kb.txt must hold its size in KB (du -sk)
; VERSION three numeric parts, e.g. 2026.10.20 (required)
; OUTFILE installer to write (default: netbeans-cnd-java-web-rust-windows-x64.exe)
; NAME    product name shown to the user (default below)

Unicode true
SetCompressor /SOLID lzma
SetCompressorDictSize 64

!ifndef SRC
  !error "pass -DSRC=<path to the unzipped netbeans/ directory>"
!endif
!ifndef VERSION
  !error "pass -DVERSION=<x.y.z>"
!endif
!ifndef OUTFILE
  !define OUTFILE "netbeans-cnd-java-web-rust-windows-x64.exe"
!endif
!ifndef NAME
  ; Not an Apache release (see PACKAGING.md), so don't call it "Apache NetBeans".
  !define NAME "NetBeans CND-Java-Web-Rust (unofficial build)"
!endif
!define DIRNAME "NetBeans-cnd-java-web-rust"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${DIRNAME}"

!include "MUI2.nsh"
!include "x64.nsh"
!include "LogicLib.nsh"

Name "${NAME}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\${DIRNAME}"
InstallDirRegKey HKLM "${UNINSTKEY}" "InstallLocation"
RequestExecutionLevel admin
ManifestDPIAware true
BrandingText "${NAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${NAME} installer"
VIAddVersionKey "LegalCopyright" "Licensed under the Apache License, Version 2.0"

!define MUI_ICON "${SRC}\nb\apache-netbeans.ico"
!define MUI_UNICON "${SRC}\nb\apache-netbeans.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\bin\netbeans64.exe"
!define MUI_FINISHPAGE_RUN_NOTCHECKED

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${SRC}\LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Function .onInit
    ${IfNot} ${RunningX64}
        MessageBox MB_OK|MB_ICONSTOP "${NAME} needs 64-bit Windows."
        Abort
    ${EndIf}
    SetRegView 64
FunctionEnd

Function un.onInit
    SetRegView 64
FunctionEnd

Section "Install"
    SetOutPath "$INSTDIR"
    File /r "${SRC}\*.*"

    WriteUninstaller "$INSTDIR\uninstall.exe"

    SetShellVarContext all
    CreateDirectory "$SMPROGRAMS\${NAME}"
    CreateShortcut "$SMPROGRAMS\${NAME}\${NAME}.lnk" "$INSTDIR\bin\netbeans64.exe" "" "$INSTDIR\nb\apache-netbeans.ico"
    CreateShortcut "$SMPROGRAMS\${NAME}\Uninstall.lnk" "$INSTDIR\uninstall.exe"
    CreateShortcut "$DESKTOP\${NAME}.lnk" "$INSTDIR\bin\netbeans64.exe" "" "$INSTDIR\nb\apache-netbeans.ico"

    WriteRegStr HKLM "${UNINSTKEY}" "DisplayName" "${NAME}"
    WriteRegStr HKLM "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKLM "${UNINSTKEY}" "Publisher" "Local build"
    WriteRegStr HKLM "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\nb\apache-netbeans.ico"
    WriteRegStr HKLM "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKLM "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKLM "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
    WriteRegDWORD HKLM "${UNINSTKEY}" "NoModify" 1
    WriteRegDWORD HKLM "${UNINSTKEY}" "NoRepair" 1
    ; size of the installed files in KB, for Apps & features
    !define /file SIZE_KB "${SRC}\..\size-kb.txt"
    WriteRegDWORD HKLM "${UNINSTKEY}" "EstimatedSize" ${SIZE_KB}
SectionEnd

Section "Uninstall"
    SetShellVarContext all
    Delete "$DESKTOP\${NAME}.lnk"
    RMDir /r "$SMPROGRAMS\${NAME}"

    ; only remove a directory this installer created
    ${If} ${FileExists} "$INSTDIR\bin\netbeans64.exe"
    ${AndIf} ${FileExists} "$INSTDIR\etc\netbeans.conf"
        RMDir /r "$INSTDIR"
    ${EndIf}

    DeleteRegKey HKLM "${UNINSTKEY}"
    ; The user directory (%APPDATA%\NetBeans) and cache
    ; (%LOCALAPPDATA%\NetBeans\Cache) are left alone on purpose.
SectionEnd
