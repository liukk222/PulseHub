#define MyAppName "PulseHub"
#define MyAppVersion "0.1.4"
#define ReleaseDir "..\target\x86_64-pc-windows-msvc\release"
#define MyAppPublisher "PulseHub contributors"
#define MyAppURL "https://github.com/liukk222/PulseHub"
#define MyAppExeName "pulsehub-config.exe"

[Setup]
AppId={{E6C012B4-3FD4-42DB-8E17-ED1594DA01E1}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={localappdata}\Programs\PulseHub
DefaultGroupName=PulseHub
DisableProgramGroupPage=yes
LicenseFile=LICENSE-AGREEMENT.txt
InfoBeforeFile=THIRD_PARTY_NOTICES.txt
OutputDir=output
OutputBaseFilename=PulseHub-Setup-{#MyAppVersion}-windows-x64
SetupIconFile=build\PulseHub.ico
UninstallDisplayIcon={app}\PulseHub.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
CloseApplications=force
RestartApplications=no
UsePreviousLanguage=no
ShowLanguageDialog=auto

[Languages]
Name: "zhcn"; MessagesFile: "build\ChineseSimplified.isl"; LicenseFile: "LICENSE-AGREEMENT.txt"; InfoBeforeFile: "THIRD_PARTY_NOTICES.txt"
Name: "english"; MessagesFile: "compiler:Default.isl"; LicenseFile: "LICENSE-AGREEMENT.txt"; InfoBeforeFile: "THIRD_PARTY_NOTICES.txt"

[CustomMessages]
zhcn.AppLanguageTitle=PulseHub 语言
zhcn.AppLanguageDescription=选择 PulseHub 界面的默认语言。
zhcn.AppLanguageSubCaption=此设置可稍后在 PulseHub 设置页面中更改。
zhcn.ChineseOption=简体中文
zhcn.EnglishOption=English
zhcn.LaunchProgram=启动 PulseHub
english.AppLanguageTitle=PulseHub language
english.AppLanguageDescription=Choose the default language for the PulseHub interface.
english.AppLanguageSubCaption=You can change this later in PulseHub Settings.
english.ChineseOption=简体中文
english.EnglishOption=English
english.LaunchProgram=Launch PulseHub

[Files]
Source: "{#ReleaseDir}\pulsehub-agent.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#ReleaseDir}\pulsehub-config.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE-AGREEMENT.txt"; DestDir: "{app}"; DestName: "LICENSE-AGREEMENT.txt"; Flags: ignoreversion
Source: "THIRD_PARTY_NOTICES.txt"; DestDir: "{app}"; DestName: "THIRD_PARTY_NOTICES.txt"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE"; Flags: ignoreversion
Source: "..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; DestName: "THIRD_PARTY_NOTICES.md"; Flags: ignoreversion
Source: "build\PulseHub.ico"; DestDir: "{app}"; DestName: "PulseHub.ico"; Flags: ignoreversion

[Icons]
Name: "{group}\PulseHub"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\PulseHub.ico"
Name: "{group}\卸载 PulseHub"; Filename: "{uninstallexe}"; Languages: zhcn
Name: "{group}\Uninstall PulseHub"; Filename: "{uninstallexe}"; Languages: english

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "PulseHub"; ValueType: string; ValueData: """{app}\pulsehub-agent.exe"" --run-agent --confirm-device-write"; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--set-ui-language {code:GetAppLanguage}"; Flags: runhidden waituntilterminated; StatusMsg: "{cm:AppLanguageDescription}"
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--shutdown-agent"; Flags: runhidden waituntilterminated; RunOnceId: "ShutdownPulseHub"

[Code]
var
  AppLanguagePage: TInputOptionWizardPage;

procedure InitializeWizard;
begin
  AppLanguagePage := CreateInputOptionPage(
    wpSelectDir,
    ExpandConstant('{cm:AppLanguageTitle}'),
    ExpandConstant('{cm:AppLanguageDescription}'),
    ExpandConstant('{cm:AppLanguageSubCaption}'),
    True,
    False);
  AppLanguagePage.Add(ExpandConstant('{cm:ChineseOption}'));
  AppLanguagePage.Add(ExpandConstant('{cm:EnglishOption}'));
  AppLanguagePage.SelectedValueIndex := 0;
  if ActiveLanguage = 'english' then
    AppLanguagePage.SelectedValueIndex := 1;
end;

function GetAppLanguage(Param: String): String;
begin
  if AppLanguagePage.SelectedValueIndex = 1 then
    Result := 'en'
  else
    Result := 'zh_cn';
end;
