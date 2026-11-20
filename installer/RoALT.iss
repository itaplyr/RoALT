; RoALT for Windows: a per-user install of the app, no admin rights.
; Build it with scripts\build-installer.ps1, which passes the version from
; src-tauri\tauri.conf.json and where cargo put the release build.

#if Ver < EncodeVer(6,6,0)
  #error Inno Setup 6.6 or newer is needed (WizardStyle dynamic windows11)
#endif
#ifndef AppVersion
  #error Pass /DAppVersion=x.y.z, or build with scripts\build-installer.ps1
#endif
; VersionInfoVersion must be numeric; a prerelease such as 0.2.0-beta.1 passes 0.2.0 here.
#ifndef AppNumVersion
  #define AppNumVersion AppVersion
#endif
#ifndef AppExe
  #define AppExe "..\src-tauri\target\release\roalt.exe"
#endif

[Setup]
AppId={{F65209C5-78F5-4735-AE83-9F83EF9FCD83}
AppName=RoALT
AppVersion={#AppVersion}
AppPublisher=inspectsoftware
AppPublisherURL=https://github.com/inspectsoftware/RoALT
VersionInfoVersion={#AppNumVersion}
; Per-user: {autopf} is %LOCALAPPDATA%\Programs.
DefaultDirName={autopf}\RoALT
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 1809, the oldest Windows Tauri 2 supports.
MinVersion=10.0.17763
WizardStyle=modern dynamic windows11
SetupIconFile=..\src-tauri\icons\icon.ico
UninstallDisplayIcon={app}\RoALT.exe
UninstallDisplayName=RoALT
; Restart Manager closes a running RoALT before its exe is replaced.
CloseApplications=force
OutputDir=..\dist-installer
OutputBaseFilename=RoALT-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ar"; MessagesFile: "compiler:Languages\Arabic.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "ko"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "pl"; MessagesFile: "compiler:Languages\Polish.isl"
Name: "pt"; MessagesFile: "compiler:Languages\Portuguese.isl"
Name: "pt_br"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "sv"; MessagesFile: "compiler:Languages\Swedish.isl"
Name: "th"; MessagesFile: "compiler:Languages\Thai.isl"
Name: "tr"; MessagesFile: "compiler:Languages\Turkish.isl"

[CustomMessages]
WebView2Missing=RoALT draws its window with the Microsoft Edge WebView2 Runtime, which is not installed.%n%nSetup will finish installing RoALT. After installing WebView2, open RoALT from the Start menu or desktop shortcut.%n%nDownload Microsoft's WebView2 installer in your browser now?
StillRunning=RoALT is running. Close it, then click OK.
UninstallDeleteTitle=Also delete your RoALT accounts and settings?
UninstallWhere=They are in %1.
UninstallWhereMoved=Delete removes only %1. You moved RoALT's data (Settings, Data): the folder you moved it to, with your saved accounts and cookies, is not deleted. Delete that folder yourself.
UninstallNotAffected=Your Roblox accounts themselves are not affected.
UninstallKeep=&Keep them%nThey are there again if you reinstall RoALT.
UninstallDelete=&Delete them%nSaved accounts, cookies, groups and settings. This cannot be undone.

de.StillRunning=RoALT wird ausgeführt. Schließen Sie es und klicken Sie dann auf OK.
de.UninstallDeleteTitle=RoALT-Konten und -Einstellungen ebenfalls löschen?
de.UninstallWhere=Sie befinden sich in %1.
de.UninstallNotAffected=Ihre Roblox-Konten selbst sind davon nicht betroffen.
de.UninstallKeep=&Behalten%nSie sind wieder da, wenn Sie RoALT erneut installieren.
de.UninstallDelete=&Löschen%nGespeicherte Konten, Cookies, Gruppen und Einstellungen. Dies kann nicht rückgängig gemacht werden.

es.StillRunning=RoALT se está ejecutando. Ciérralo y haz clic en Aceptar.
es.UninstallDeleteTitle=¿Eliminar también tus cuentas y ajustes de RoALT?
es.UninstallWhere=Están en %1.
es.UninstallNotAffected=Tus cuentas de Roblox en sí no se ven afectadas.
es.UninstallKeep=&Conservarlos%nVolverán a estar ahí si reinstalas RoALT.
es.UninstallDelete=&Eliminarlos%nCuentas guardadas, cookies, grupos y ajustes. Esto no se puede deshacer.

fr.StillRunning=RoALT est en cours d'exécution. Fermez-le, puis cliquez sur OK.
fr.UninstallDeleteTitle=Supprimer aussi vos comptes et paramètres RoALT ?
fr.UninstallWhere=Ils se trouvent dans %1.
fr.UninstallNotAffected=Vos comptes Roblox eux-mêmes ne sont pas affectés.
fr.UninstallKeep=&Les conserver%nIls seront de nouveau là si vous réinstallez RoALT.
fr.UninstallDelete=&Les supprimer%nComptes enregistrés, cookies, groupes et paramètres. Cette action est irréversible.

pt.StillRunning=O RoALT está em execução. Feche-o e clique em OK.
pt.UninstallDeleteTitle=Eliminar também as suas contas e definições do RoALT?
pt.UninstallWhere=Estão em %1.
pt.UninstallNotAffected=As suas contas Roblox em si não são afetadas.
pt.UninstallKeep=&Mantê-los%nVoltam a estar disponíveis se reinstalar o RoALT.
pt.UninstallDelete=&Eliminá-los%nContas guardadas, cookies, grupos e definições. Isto não pode ser anulado.

pt_br.StillRunning=O RoALT está em execução. Feche-o e clique em OK.
pt_br.UninstallDeleteTitle=Excluir também suas contas e configurações do RoALT?
pt_br.UninstallWhere=Eles estão em %1.
pt_br.UninstallNotAffected=Suas contas do Roblox em si não são afetadas.
pt_br.UninstallKeep=&Mantê-los%nEles estarão lá novamente se você reinstalar o RoALT.
pt_br.UninstallDelete=&Excluí-los%nContas salvas, cookies, grupos e configurações. Isso não pode ser desfeito.

ru.StillRunning=RoALT запущен. Закройте его и нажмите OK.
ru.UninstallDeleteTitle=Также удалить ваши аккаунты и настройки RoALT?
ru.UninstallWhere=Они находятся в %1.
ru.UninstallNotAffected=Сами аккаунты Roblox не будут затронуты.
ru.UninstallKeep=&Оставить%nОни снова появятся при повторной установке RoALT.
ru.UninstallDelete=&Удалить%nСохранённые аккаунты, куки, группы и настройки. Это действие нельзя отменить.

tr.StillRunning=RoALT çalışıyor. Kapatın ve Tamam'a tıklayın.
tr.UninstallDeleteTitle=RoALT hesaplarınız ve ayarlarınız da silinsin mi?
tr.UninstallWhere=%1 klasöründe bulunuyorlar.
tr.UninstallNotAffected=Roblox hesaplarınızın kendisi etkilenmez.
tr.UninstallKeep=&Sakla%nRoALT'ı yeniden yüklerseniz tekrar orada olurlar.
tr.UninstallDelete=&Sil%nKayıtlı hesaplar, çerezler, gruplar ve ayarlar. Bu işlem geri alınamaz.

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; DestName: "RoALT.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\RoALT"; Filename: "{app}\RoALT.exe"
Name: "{autodesktop}\RoALT"; Filename: "{app}\RoALT.exe"; Tasks: desktopicon

; roalt:// links (a shared game or server) open RoALT on Play with it filled in.
[Registry]
Root: HKA; Subkey: "Software\Classes\roalt"; ValueType: string; ValueData: "URL:RoALT link"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\roalt"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\roalt\DefaultIcon"; ValueType: string; ValueData: """{app}\RoALT.exe"",0"
Root: HKA; Subkey: "Software\Classes\roalt\shell\open\command"; ValueType: string; ValueData: """{app}\RoALT.exe"" ""%1"""

[Run]
Filename: "{app}\RoALT.exe"; Description: "{cm:LaunchProgram,RoALT}"; Flags: nowait postinstall skipifsilent; Check: WebView2Installed
; RoALT's self-updater runs Setup with /VERYSILENT ... /RELAUNCH and quits: start it again.
Filename: "{app}\RoALT.exe"; Flags: nowait; Check: Relaunch

[Code]
const
  WebView2Key = 'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  { Microsoft's Evergreen Bootstrapper: Setup only opens the link, it downloads nothing itself. }
  WebView2Download = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703';
  { tauri-plugin-single-instance holds <identifier>-sim while RoALT runs. }
  RunningMutex = 'com.inspectsoftware.roalt-sim';

function HasWebView2(Root: Integer): Boolean;
var
  Version: String;
begin
  Result := RegQueryStringValue(Root, WebView2Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0');
end;

{ The machine-wide runtime (HKLM32 is WOW6432Node) or a per-user one. }
function WebView2Installed: Boolean;
begin
  Result := HasWebView2(HKLM32) or HasWebView2(HKCU);
end;

function InitializeSetup: Boolean;
var
  Code: Integer;
begin
  if not WebView2Installed and (SuppressibleMsgBox(
      CustomMessage('WebView2Missing'),
      mbConfirmation, MB_YESNO, IDNO) = IDYES) then
    ShellExecAsOriginalUser('open', WebView2Download, '', '', SW_SHOWNORMAL, ewNoWait, Code);
  Result := True;
end;

{ Setup was started by RoALT's self-updater (/RELAUNCH), silently. }
function Relaunch: Boolean;
var
  I: Integer;
begin
  Result := False;
  if WizardSilent and WebView2Installed then
    for I := 1 to ParamCount do
      if CompareText(ParamStr(I), '/RELAUNCH') = 0 then
        Result := True;
end;

{ A running RoALT holds its exe and database open: uninstalling then would leave both behind. }
function InitializeUninstall: Boolean;
begin
  Result := True;
  while Result and CheckForMutexes(RunningMutex) do
    Result := SuppressibleMsgBox(CustomMessage('StillRunning'), mbError, MB_OKCANCEL, IDCANCEL) = IDOK;
end;

{ RoALT opened Roblox's links (Settings, Default Roblox launcher): give them back to the program it
  took them from (RoALTBefore), else drop RoALT's so Windows falls back to an all-users Roblox. }
procedure RestoreRobloxLinks;
var
  Schemes: array[0..1] of String;
  I: Integer;
  Key, Command, Before: String;
begin
  Schemes[0] := 'roblox-player';
  Schemes[1] := 'roblox';
  for I := 0 to 1 do begin
    Key := 'Software\Classes\' + Schemes[I];
    if RegQueryStringValue(HKCU, Key + '\shell\open\command', '', Command) and
        (Pos(Lowercase(ExpandConstant('{app}\RoALT.exe')), Lowercase(Command)) > 0) then begin
      if RegQueryStringValue(HKCU, Key, 'RoALTBefore', Before) then
        RegWriteStringValue(HKCU, Key + '\shell\open\command', '', Before)
      else
        RegDeleteKeyIncludingSubkeys(HKCU, Key);
    end;
    RegDeleteValue(HKCU, Key, 'RoALTBefore');
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data, Where: String;
begin
  if CurUninstallStep = usUninstall then
    RestoreRobloxLinks;
  Data := ExpandConstant('{localappdata}\RoALT');
  if (CurUninstallStep <> usPostUninstall) or UninstallSilent or not DirExists(Data) then
    exit;
  Where := FmtMessage(CustomMessage('UninstallWhere'), [Data]);
  { Settings > Data moved the database (accounts, cookies) to a folder of the user's choice, named
    in location.json. It can hold other files too, so it is never deleted here: say so. Not when
    the database is here after all (moved away and back: location.json stays). }
  if FileExists(Data + '\location.json') and not FileExists(Data + '\roalt.db') then
    Where := FmtMessage(CustomMessage('UninstallWhereMoved'), [Data]);
  { Keep is the first button, so the default: TaskDialogMsgBox has no default-button flag. }
  if TaskDialogMsgBox(CustomMessage('UninstallDeleteTitle'),
      Where + ' ' + CustomMessage('UninstallNotAffected'),
      mbConfirmation, MB_YESNO, [CustomMessage('UninstallKeep'), CustomMessage('UninstallDelete')], 0) = IDNO then begin
    DelTree(Data, True, True, True);
    { The window position tauri-plugin-window-state keeps. }
    DelTree(ExpandConstant('{userappdata}\com.inspectsoftware.roalt'), True, True, True);
  end;
end;
