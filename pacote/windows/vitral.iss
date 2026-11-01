; Instalador do Vitral para Windows (Inno Setup 6): só para o usuário, sem administrador, em
; %LOCALAPPDATA%\Programs\Vitral, no mesmo layout que o atualizador usa (distribuicao.md, seção 3.8):
;
;   Vitral.exe        o lançador (vitral-lancador.exe), para onde apontam o atalho e o pino da barra
;   versoes.txt       "atual=<versão>" e "anterior=<a que estava>"
;   app-<versão>\     o pacote inteiro desta versão (vitral.exe, o lançador e os textos)
;
; O instalador só cria a primeira app-<versão>, o lançador, o versoes.txt, o atalho e o
; desinstalador; trocar de versão depois é com o próprio Vitral.
;
; No release.yml: ISCC /DVersao=0.3.0 /DOrigem=<pasta do pacote> /O<saída> pacote\windows\vitral.iss
; Os textos que aparecem vêm do BrazilianPortuguese.isl (os {cm:…}): este arquivo fica só em ASCII
; fora dos comentários, que o ISCC lê sem BOM como ANSI.
#ifndef Versao
  #error "Passe /DVersao=X.Y.Z"
#endif
#ifndef Origem
  #error "Passe /DOrigem=<pasta com o vitral.exe e o vitral-lancador.exe>"
#endif
; A versão do Windows só tem números (0.3.0-rc.5 vira 0.3.0.0); a de texto fica inteira.
#if Pos("-", Versao) > 0
  #define VersaoNumerica Copy(Versao, 1, Pos("-", Versao) - 1)
#else
  #define VersaoNumerica Versao
#endif

[Setup]
; O AppId nunca muda entre versões (é o que liga o instalador novo ao velho e ao desinstalador).
AppId={{DE776FCA-34A5-4442-A784-DC2E5BDEC95F}
AppName=Vitral
AppVersion={#Versao}
AppVerName=Vitral {#Versao}
AppPublisher=Victor Menegate
AppPublisherURL=https://github.com/VictorMenegate/vitral
AppSupportURL=https://github.com/VictorMenegate/vitral/issues
AppUpdatesURL=https://github.com/VictorMenegate/vitral/releases
VersionInfoVersion={#VersaoNumerica}
VersionInfoTextVersion={#Versao}
VersionInfoProductName=Vitral
VersionInfoCompany=VictorMenegate
DefaultDirName={localappdata}\Programs\Vitral
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputBaseFilename=vitral-{#Versao}-windows-x86_64-instalador
SetupIconFile=..\..\assets\vitral.ico
UninstallDisplayIcon={app}\Vitral.exe
UninstallDisplayName=Vitral
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; Fecha um Vitral aberto que use um arquivo que vai ser trocado (reinstalar a mesma versão com ele
; aberto), sem reabrir sozinho.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "atalho"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; app-<versão> fica igual ao que o atualizador extrai do .tar.gz da mesma versão.
Source: "{#Origem}\*"; DestDir: "{app}\app-{#Versao}"; Flags: ignoreversion recursesubdirs
Source: "{#Origem}\vitral-lancador.exe"; DestDir: "{app}"; DestName: "Vitral.exe"; Flags: ignoreversion

[Icons]
; O mesmo AppUserModelID que o app declara (set_app_identity em src/main.rs): agrupa as janelas na
; barra, dá nome e ícone às notificações, e o pino da barra aponta para o lançador (nunca para a
; app-<versão>, que muda). Abre na pasta do usuário, como o .desktop do Linux abre na casa.
Name: "{autoprograms}\Vitral"; Filename: "{app}\Vitral.exe"; WorkingDir: "{%USERPROFILE}"; AppUserModelID: "VictorMenegate.Vitral"
Name: "{autodesktop}\Vitral"; Filename: "{app}\Vitral.exe"; WorkingDir: "{%USERPROFILE}"; AppUserModelID: "VictorMenegate.Vitral"; Tasks: atalho

[Registry]
; O nome das notificações sem pacote (o GPUI grava o mesmo ao mostrar uma): é do programa, sai com ele.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\VictorMenegate.Vitral"; ValueType: string; ValueName: "DisplayName"; ValueData: "Vitral"; Flags: uninsdeletekey

[Run]
Filename: "{app}\Vitral.exe"; Description: "{cm:LaunchProgram,Vitral}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Só o programa, com o que o atualizador deixar: as versões, o download pela metade, as extrações
; que não terminaram, as trocas atômicas e o estado. Os dados (%LOCALAPPDATA%\vitral) e a
; configuração (%APPDATA%\vitral) ficam. Nunca a pasta inteira: um /DIR= errado apagaria o que não
; é do Vitral.
Type: filesandordirs; Name: "{app}\app-*"
Type: filesandordirs; Name: "{app}\.novo-*"
Type: filesandordirs; Name: "{app}\.baixando"
Type: files; Name: "{app}\versoes.txt"
Type: files; Name: "{app}\versoes.txt.novo"
Type: files; Name: "{app}\Vitral.exe.novo"
Type: files; Name: "{app}\estado.json"
Type: files; Name: "{app}\.*.tmp"
Type: files; Name: "{app}\.trava"
Type: dirifempty; Name: "{app}"

[Code]
var
  VersoesFalhou: Boolean;

// O valor de "chave=" nas linhas, ou '' (o lançador lê o mesmo formato: lancador/src/escolha.rs).
function ValorDe(const Linhas: TArrayOfString; const Chave: String): String;
var
  I: Integer;
  Linha: String;
begin
  Result := '';
  for I := 0 to GetArrayLength(Linhas) - 1 do
  begin
    Linha := Trim(Linhas[I]);
    if Pos(Chave + '=', Linha) = 1 then
      Result := Trim(Copy(Linha, Length(Chave) + 2, Length(Linha)));
  end;
end;

// Grava ao lado e troca de uma vez. Se cair no meio, o lançador abre a maior app-* que tiver o
// vitral.exe.
function TrocarVersoes(const Arquivo, Texto: String): Boolean;
begin
  Result := False;
  if not SaveStringToFile(Arquivo + '.novo', Texto, False) then
    Exit;
  if FileExists(Arquivo) then
    if not DeleteFile(Arquivo) then
      Exit;
  Result := RenameFile(Arquivo + '.novo', Arquivo);
end;

// Aponta o versoes.txt para a versão instalada; a que estava vira a anterior (reinstalar a mesma
// guarda a anterior de antes).
procedure CurStepChanged(CurStep: TSetupStep);
var
  Linhas: TArrayOfString;
  Atual, Anterior, Texto, Arquivo: String;
begin
  if CurStep = ssPostInstall then
  begin
    Arquivo := ExpandConstant('{app}\versoes.txt');
    Atual := '';
    Anterior := '';
    if LoadStringsFromFile(Arquivo, Linhas) then
    begin
      Atual := ValorDe(Linhas, 'atual');
      Anterior := ValorDe(Linhas, 'anterior');
    end;
    if (Atual <> '') and (Atual <> '{#Versao}') then
      Anterior := Atual;
    Texto := 'atual={#Versao}' + #13#10;
    if (Anterior <> '') and (Anterior <> '{#Versao}') then
      Texto := Texto + 'anterior=' + Anterior + #13#10;
    Log('versoes.txt: ' + Texto);
    if not TrocarVersoes(Arquivo, Texto) then
    begin
      // Sem acento fora dos comentários (ver o topo): os #nnn são as letras acentuadas.
      VersoesFalhou := True;
      Log('versoes.txt: a troca falhou');
      SuppressibleMsgBox('N'#227'o consegui gravar o ' + Arquivo + ': o Vitral pode abrir a vers'#227'o que j'#225' estava. Instale de novo.', mbError, MB_OK, IDOK);
    end;
  end;
end;

// A instalação silenciosa (/VERYSILENT) sai com 1 quando o versoes.txt não foi trocado.
function GetCustomSetupExitCode: Integer;
begin
  Result := 0;
  if VersoesFalhou then
    Result := 1;
end;
