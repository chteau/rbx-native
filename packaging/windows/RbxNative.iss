; RbxNative's Windows setup (Inno Setup 6). Per user, no admin: it installs
; to %LOCALAPPDATA%\Programs\RbxNative, which the editor can write to, so the
; auto-updater keeps working and moves DisplayVersion along
; (crates/rbx_studio/src/updater/compat.rs, which also holds the AppId).
;
; Built by .github/release/package.sh:
;   ISCC /DVersion=<version> /DSource=<staged package folder>
;        /DOutput=<out dir> /DRepo=<repository root> RbxNative.iss

#ifndef Version
  #error Pass /DVersion=<version>
#endif

[Setup]
AppId={{39CCCF6D-DAD2-464F-A1AB-97143C3F610D}
AppName=RbxNative
AppVersion={#Version}
AppVerName=RbxNative {#Version}
AppPublisher=chteau
AppPublisherURL=https://rbxnative.chteau.bzh
AppSupportURL=https://github.com/chteau/rbx-native/issues
AppUpdatesURL=https://github.com/chteau/rbx-native/releases
DefaultDirName={autopf}\RbxNative
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#Output}
OutputBaseFilename=RbxNative-{#Version}-windows-x64-setup
SetupIconFile={#Repo}\crates\rbx_studio\windows\rbxstudio.ico
UninstallDisplayIcon={app}\RbxNative.exe
UninstallDisplayName=RbxNative
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ChangesAssociations=yes
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "associate"; Description: "Open .rbxl and .rbxlx places with RbxNative"; GroupDescription: "File types:"

[Files]
Source: "{#Source}\RbxNative.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\*.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\*.html"; DestDir: "{app}"; Flags: ignoreversion
; Play's network simulator. A loaded driver can't be overwritten, and these
; never change between releases.
Source: "{#Source}\WinDivert.dll"; DestDir: "{app}"; Flags: onlyifdoesntexist
Source: "{#Source}\WinDivert64.sys"; DestDir: "{app}"; Flags: onlyifdoesntexist

[Icons]
Name: "{autoprograms}\RbxNative"; Filename: "{app}\RbxNative.exe"
Name: "{autodesktop}\RbxNative"; Filename: "{app}\RbxNative.exe"; Tasks: desktopicon

[Registry]
; Lets Win+R and `start RbxNative` find it.
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\RbxNative.exe"; ValueType: string; ValueName: ""; ValueData: "{app}\RbxNative.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\RbxNative.Place"; ValueType: string; ValueName: ""; ValueData: "Roblox place"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\RbxNative.Place\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\RbxNative.exe"",0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\RbxNative.Place\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\RbxNative.exe"" ""%1"""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.rbxl"; ValueType: string; ValueName: ""; ValueData: "RbxNative.Place"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.rbxl\OpenWithProgids"; ValueType: string; ValueName: "RbxNative.Place"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.rbxlx"; ValueType: string; ValueName: ""; ValueData: "RbxNative.Place"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.rbxlx\OpenWithProgids"; ValueType: string; ValueName: "RbxNative.Place"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate

[Run]
Filename: "{app}\RbxNative.exe"; Description: "{cm:LaunchProgram,RbxNative}"; Flags: nowait postinstall skipifsilent

; What the updater leaves between launches.
[UninstallDelete]
Type: files; Name: "{app}\RbxNative.old.exe"
Type: filesandordirs; Name: "{app}\.rbxstudio-update-*"
