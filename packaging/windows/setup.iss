; All-users per-app installers. Shared executables have one fixed path and reference-counted
; uninstall records; never touch user data or start an application from this elevated process.
#if Ver < EncodeVer(7, 1, 1)
  #error Inno Setup 7.1.1 is required
#endif

[Setup]
AppId=io.github.mahype.{#AppIdentity}
AppName={#AppTitle}
AppVersion={#AppVersion}
VersionInfoVersion={#ResourceVersion}
AppPublisher=TorroWorkplace contributors
AppPublisherURL=https://github.com/mahype/TorroWorkplace
DefaultDirName={autopf64}\TorroWorkplace
DisableDirPage=yes
UsePreviousAppDir=no
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallFilesDir={app}\uninstall\{#AppIdentity}
UninstallLogMode=append
UninstallDisplayIcon={app}\{#AppProgram}.exe
DisableProgramGroupPage=yes
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
CloseApplicationsFilter=*.exe
RestartApplications=no
AllowCancelDuringInstall=yes
SetupLogging=yes

[Dirs]
; Inno processes directories AFTER Restart Manager and calls ProcessEvents immediately afterwards,
; before uninstall information or payload files. Even an existing directory runs this callback.
Name: "{app}"; BeforeInstall: CheckReadyToCopy

[Files]
#include AddBackslash(Payload) + "files.iss"

[Icons]
Name: "{commonprograms}\Torro Workplace\{#AppTitle}"; Filename: "{app}\{#AppProgram}.exe"; Parameters: "{#AppArgs}"

[Code]
var
  FatalFailure: Boolean;

function InitializeSetup(): Boolean;
var
  Index: Integer;
begin
  Result := True;
  for Index := 1 to ParamCount do
    { Inno also reads NoCancel=yes from a loaded INF; checking /NOCANCEL alone is insufficient. }
    if (Pos('/nocancel', Lowercase(ParamStr(Index))) = 1) or
      (Pos('/loadinf', Lowercase(ParamStr(Index))) = 1) then begin
      Result := False;
      Log('/NOCANCEL and /LOADINF are not supported: installer safety requires native cancellation.');
    end;
end;

procedure CancelButtonClick(CurPageID: Integer; var Cancel, Confirm: Boolean);
begin
  if FatalFailure then begin
    Cancel := True;
    Confirm := False;
  end;
end;

procedure FailInstallation(Reason: String);
begin
  { Inno swallows exceptions in Before/AfterInstall. Set its native NeedToAbortInstall instead:
    WizardForm.Close -> MainForm.Close -> CancelButtonClick -> native ProcessEvents -> fatal abort.
    Show/enable the Cancel control also in /VERYSILENT so MainForm.Close's guards hold. }
  if FatalFailure then exit;
  FatalFailure := True;
  Log(Reason);
  WizardForm.Show;
  WizardForm.Enabled := True;
  WizardForm.CancelButton.Visible := True;
  WizardForm.CancelButton.Enabled := True;
  WizardForm.Close;
end;

function CanCopy(): Boolean;
begin
  { Also blocks the next sidecar if a per-file verification failed after the preceding copy. }
  Result := not FatalFailure;
end;

function OpenForPreflight(FileName: String; Access, ShareMode: LongWord;
  Security: NativeUInt; Creation, Flags: LongWord; Template: NativeUInt): NativeUInt;
  external 'CreateFileW@kernel32.dll stdcall';
function ClosePreflightHandle(Handle: NativeUInt): Boolean;
  external 'CloseHandle@kernel32.dll stdcall';

function FixedDestination(): Boolean;
begin
  { DisableDirPage is cosmetic: /DIR and /LOADINF must not split shared-file ownership. }
  Result := CompareText(RemoveBackslashUnlessRoot(ExpandFileName(ExpandConstant('{app}'))),
    RemoveBackslashUnlessRoot(ExpandFileName(ExpandConstant('{autopf64}\TorroWorkplace')))) = 0;
end;

function MarkerText(ProgramName: String): String;
var
  ProgramPath: String;
begin
  ProgramPath := ExpandConstant('{autopf64}\TorroWorkplace\') + ProgramName + '.exe';
  StringChangeEx(ProgramPath, '\', '\\', True);
  Result := '{"schema": 1, "manager": "setup", "package": "' + ProgramName + '", "program": "' + ProgramPath + '"}';
end;

function VersionParts(Value: String; var Parts: TArrayOfInteger): Boolean;
var
  Index, Dot, Number: Integer;
  Piece, Suffix: String;
begin
  Result := False;
  SetArrayLength(Parts, 5);
  Dot := Pos('-rc.', Value);
  Parts[3] := 1;
  Parts[4] := 0;
  if Dot > 0 then begin
    Suffix := Copy(Value, Dot + 4, Length(Value));
    Parts[4] := StrToIntDef(Suffix, -1);
    if Parts[4] < 1 then exit;
    Parts[3] := 0;
    Value := Copy(Value, 1, Dot - 1);
  end;
  for Index := 0 to 2 do begin
    Dot := Pos('.', Value);
    if Index < 2 then begin
      if Dot = 0 then exit;
      Piece := Copy(Value, 1, Dot - 1);
      Delete(Value, 1, Dot);
    end else Piece := Value;
    Number := StrToIntDef(Piece, -1);
    if Number < 0 then exit;
    Parts[Index] := Number;
  end;
  Result := True;
end;

function NotNewer(OldVersion, NewVersion: String): Boolean;
var
  OldParts, NewParts: TArrayOfInteger;
  Index: Integer;
begin
  Result := False;
  if not VersionParts(Trim(OldVersion), OldParts) then exit;
  if not VersionParts(NewVersion, NewParts) then exit;
  for Index := 0 to 4 do begin
    if OldParts[Index] > NewParts[Index] then exit;
    if OldParts[Index] < NewParts[Index] then begin Result := True; exit; end;
  end;
  Result := True;
end;

function CheckProgram(ProgramName: String): Boolean;
var
  Path, OldVersion: String;
  OldMarker: AnsiString;
begin
  Path := ExpandConstant('{app}\') + ProgramName;
  Result := True;
  if DirExists(Path + '.exe') or DirExists(Path + '.exe.torro-package.json') or DirExists(Path + '.version') then begin
    Result := False;
    exit;
  end;
  if FileExists(Path + '.exe') or FileExists(Path + '.exe.torro-package.json') or FileExists(Path + '.version') then begin
    Result := LoadStringFromFile(Path + '.exe.torro-package.json', OldMarker) and
      LoadStringFromFile(Path + '.version', OldMarker);
    if not Result then exit;
    OldVersion := String(OldMarker);
    Result := NotNewer(OldVersion, '{#AppVersion}');
    if not Result then exit;
    Result := LoadStringFromFile(Path + '.exe.torro-package.json', OldMarker);
    if Result then Result := Trim(String(OldMarker)) = MarkerText(ProgramName);
  end;
end;

function Preflight(): Boolean;
begin
  Result := True;
#include AddBackslash(Payload) + "checks.iss"
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not FixedDestination() then
    Result := 'Setup requires the fixed all-users Program Files\TorroWorkplace directory. Custom /DIR or saved installation paths are not supported.'
  else if not Preflight() then
    Result := 'Setup cannot replace a newer version, a damaged installation or files from another installer. Remove that installation first. No application files have been changed.';
end;

procedure CheckUnlocked(FileName: String);
var
  Handle: NativeUInt;
begin
  if FatalFailure then exit;
  FileName := ExpandConstant('{app}\') + FileName;
  if not FileExists(FileName) then exit;
  { Opening without truncation tests actual write/exclusive access, including mapped executables.
    This runs AFTER Restart Manager's close attempt, BEFORE the first payload copy. }
  Handle := OpenForPreflight(FileName, $40000000, 0, 0, 3, 0, 0);
  if Handle = NativeUInt(-1) then begin
    FailInstallation('Setup was cancelled because an application file is still in use or not writable. Close the apps and background service, then run Setup again.');
    exit;
  end;
  if not ClosePreflightHandle(Handle) then
    FailInstallation('Could not close the preflight file handle. Setup was cancelled.');
end;

procedure CheckReadyToCopy();
begin
  if FatalFailure then exit;
  try
    if not FixedDestination() or not Preflight() then begin
      FailInstallation('The installation destination or ownership changed. Setup was cancelled.');
      exit;
    end;
#include AddBackslash(Payload) + "locks.iss"
  except
    FailInstallation('Could not complete the installation preflight. Setup was cancelled.');
  end;
end;

procedure VerifyProgram(ProgramName, ExpectedHash: String);
begin
  { Never let Skip advance a sidecar beyond its executable. Hash/read failures must take the
    native cancellation path too, not escape as a swallowed callback exception. }
  try
    if GetSHA256OfFile(ExpandConstant('{app}\') + ProgramName + '.exe') <> ExpectedHash then
      FailInstallation('An application file was not installed correctly. Setup was cancelled.');
  except
    FailInstallation('Could not verify an application file. Setup was cancelled.');
  end;
end;

procedure VerifyVersion(ProgramName: String);
var
  Value: AnsiString;
begin
  try
    if not LoadStringFromFile(ExpandConstant('{app}\') + ProgramName + '.version', Value) then
      FailInstallation('Could not verify the installed version. Setup was cancelled.');
    if Trim(String(Value)) <> '{#AppVersion}' then
      FailInstallation('The installed version does not match the executable. Setup was cancelled.');
  except
    FailInstallation('Could not verify the installed version. Setup was cancelled.');
  end;
end;

procedure WriteMarker(ProgramName: String);
begin
  try
    if not SaveStringToFile(ExpandConstant('{app}\') + ProgramName + '.exe.torro-package.json', MarkerText(ProgramName), False) then
      FailInstallation('Could not write the native installer marker. Setup was cancelled.');
  except
    FailInstallation('Could not write the native installer marker. Setup was cancelled.');
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then begin
    { Inno normally continues when the person declines closure. Abort before PerformInstall,
      rather than reaching a Skip dialog after another shared executable was already replaced. }
    if not FixedDestination() then RaiseException('Custom installation destinations are not supported.');
    if WizardForm.PreparingNoRadio.Visible and WizardForm.PreparingNoRadio.Checked then
      RaiseException('Setup was cancelled because closing applications was declined. No application files have been changed.');
  end;
end;
