#ifndef NativeRoot
  #error NativeRoot must point to the allowlisted native runtime package.
#endif
#ifndef NativeFiles
  #error NativeFiles must point to the generated file entries.
#endif
#ifndef AppVersion
  #error AppVersion must match VERSION.
#endif

[Setup]
AppId=CodexUsageMonitorNativeBootstrap
AppName=Codex Usage Monitor
AppVersion={#AppVersion}
AppVerName=Codex Usage Monitor {#AppVersion} (Rust)
AppCopyright=MIT License; see NOTICE.md
DefaultDirName={localappdata}\Programs\CodexUsageMonitor
CreateAppDir=no
Uninstallable=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
MinVersion=10.0.19041
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableWelcomePage=no
WizardStyle=modern
SetupIconFile=..\native\app.ico
LicenseFile=..\LICENSE
OutputDir=..\dist\release
OutputBaseFilename=CodexUsageMonitor-Setup-{#AppVersion}-x64
Compression=lzma2
SolidCompression=yes
CloseApplications=no
RestartApplications=no
RestartIfNeededByRun=no
UninstallRestartComputer=no

[Languages]
Name: "chinesesimp"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
chinesesimp.StartupTask=登录 Windows 时启动工具，并由工具打开 Codex
english.StartupTask=Start the monitor at Windows sign-in and open Codex through it
chinesesimp.StartNow=立即启动 Codex Usage Monitor
english.StartNow=Start Codex Usage Monitor now
chinesesimp.InstallStatus=正在验证 Codex 并安装原生运行环境...
english.InstallStatus=Checking Codex and installing the native runtime...
chinesesimp.InstallError=原生安装失败，未启动 Codex。请确认已安装完整的 Codex Desktop，再重试。
english.InstallError=Native installation failed. Codex was not started. Check the Codex Desktop installation and try again.
chinesesimp.FinishInfo=工具已安装，官方订阅无需 API Key。%n%n完成后将自动连接 Codex；如果 Codex 已从其他入口运行且未开放连接，请正常退出它，再使用新的桌面快捷方式。工具不会强制重启 Codex。%n%n可从 Windows 设置中的“已安装的应用”卸载，用量记录和加密凭据默认保留。
english.FinishInfo=The monitor is installed. Official subscription monitoring needs no API key.%n%nThe monitor will connect to Codex. If Codex is already running without a debug connection, exit it normally and use the new desktop shortcut. Codex is never forcibly restarted.%n%nUninstall from Windows Settings. Usage records and encrypted credentials are kept by default.

[Tasks]
Name: "autostart"; Description: "{cm:StartupTask}"

[Files]
#include NativeFiles

[Run]
Filename: "{localappdata}\Programs\CodexUsageMonitor\CodexUsageMonitor.exe"; Description: "{cm:StartNow}"; Flags: postinstall nowait; Check: ShouldLaunch

[Code]
function IsIsolatedInstall: Boolean;
begin
  Result := ExpandConstant('{param:NATIVEINSTALLROOT|}') <> '';
end;

function ShouldLaunch: Boolean;
begin
  Result := not IsIsolatedInstall;
  if WizardSilent then
    Result := Result and (ExpandConstant('{param:LAUNCH|0}') = '1');
end;

procedure RunNative(const Parameters: String);
var
  Code: Integer;
begin
  if not Exec(ExpandConstant('{tmp}\CodexUsageMonitorPayload\CodexUsageMonitor.exe'),
      Parameters, '', SW_HIDE, ewWaitUntilTerminated, Code) then
    RaiseException(ExpandConstant('{cm:InstallError}'));
  if Code <> 0 then
    RaiseException(ExpandConstant('{cm:InstallError}'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Parameters: String;
  Code: Integer;
begin
  if CurStep = ssPostInstall then begin
    WizardForm.StatusLabel.Caption := ExpandConstant('{cm:InstallStatus}');
    if not IsIsolatedInstall then begin
      Parameters := '--prepare-install';
      if WizardSilent then Parameters := Parameters + ' --quiet';
      RunNative(Parameters);
    end;
    Parameters := '--install --quiet';
    if IsIsolatedInstall then
      Parameters := Parameters + ' --install-root "' + ExpandConstant('{param:NATIVEINSTALLROOT|}') + '"'
    else if WizardIsTaskSelected('autostart') then
      Parameters := Parameters + ' --enable-autostart';
    RunNative(Parameters);
    if not IsIsolatedInstall and not WizardIsTaskSelected('autostart') then begin
      if not Exec(ExpandConstant('{localappdata}\Programs\CodexUsageMonitor\CodexUsageMonitor.exe'),
          '--disable-autostart', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then
        RaiseException(ExpandConstant('{cm:InstallError}'));
    end;
    WizardForm.FinishedLabel.Caption := ExpandConstant('{cm:FinishInfo}');
  end;
end;
