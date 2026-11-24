; Inno Setup script (https://jrsoftware.org/isinfo.php). Build on Windows or in CI:
;   iscc /DAppVersion=1.0.0 dist\windows\joebro.iss      (expects dist\out\JoeBro.exe)
#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif
[Setup]
AppId={{6C2B5E42-0D5B-4B0A-9E55-4A0EB0000001}
AppName=JoeBro
AppVersion={#AppVersion}
AppPublisher=JoeBro
DefaultDirName={localappdata}\Programs\JoeBro
DefaultGroupName=JoeBro
PrivilegesRequired=lowest
OutputDir=..\out
OutputBaseFilename=JoeBro-{#AppVersion}-windows-x86_64-setup
SetupIconFile=..\..\assets\icon\joebro.ico
UninstallDisplayIcon={app}\JoeBro.exe
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
[Files]
Source: "..\out\JoeBro.exe"; DestDir: "{app}"; Flags: ignoreversion
[Icons]
Name: "{group}\JoeBro"; Filename: "{app}\JoeBro.exe"
Name: "{userdesktop}\JoeBro"; Filename: "{app}\JoeBro.exe"; Tasks: desktopicon
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
[Run]
Filename: "{app}\JoeBro.exe"; Description: "Start JoeBro"; Flags: nowait postinstall skipifsilent
