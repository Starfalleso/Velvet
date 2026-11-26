#define MyAppName "Velvet"
#define MyAppVersion "0.1.0"
#define MyAppPublisher "Starfalleso"
#define MyAppURL "https://github.com/Starfalleso/Velvet"
#define MyAppExeName "velvet.exe"

[Setup]
AppId={{9F3B8A7C-6E41-4A1D-9A2E-7C5D8B1E4F20}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline
OutputDir=..\dist
OutputBaseFilename=Velvet-Setup-{#MyAppVersion}
SetupIconFile=..\assets\velvet.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Start Velvet automatically with Windows"; GroupDescription: "Integration:"
Name: "yasb"; Description: "Add / update Velvet button on YASB top bar"; GroupDescription: "Integration:"; Check: HasYasbConfig

[Files]
Source: "..\target\dist\velvet.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\{#MyAppName}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--enable-autostart"; Tasks: autostart; Flags: runhidden
Filename: "{app}\{#MyAppExeName}"; Parameters: "--setup-yasb"; Tasks: yasb; Flags: runhidden
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--quit"; RunOnceId: "VelvetQuit"; Flags: runhidden skipifdoesntexist
Filename: "{app}\{#MyAppExeName}"; Parameters: "--disable-autostart"; RunOnceId: "VelvetDisableAutostart"; Flags: runhidden skipifdoesntexist

[Code]
function HasYasbConfig: Boolean;
begin
  Result := FileExists(GetEnv('USERPROFILE') + '\.config\yasb\config.yaml');
end;

procedure StopRunningVelvet;
var
  ResultCode: Integer;
  ExePath: String;
begin
  ExePath := ExpandConstant('{app}\{#MyAppExeName}');
  if FileExists(ExePath) then
  begin
    Exec(ExePath, '--quit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    Sleep(250);
  end;
  Exec('taskkill.exe', '/F /IM velvet.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    StopRunningVelvet();
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    StopRunningVelvet();
end;
