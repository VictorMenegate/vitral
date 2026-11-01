# O pacote do Windows num Windows de verdade (o runner windows-2025 do GitHub), sem ninguém olhando.
# Roda no .github/workflows/windows-prints.yml com os pacotes de um release.yml (workflow_dispatch sem
# tag, que não publica nada).
#
# Uso: ferramentas/instalador-windows.ps1 <pasta com o .zip, o .tar.gz e o -instalador.exe> <pasta_dos_prints>
#
# Na ordem: confere o zip e o .tar.gz (o mesmo conteúdo); fotografa o instalador aberto (em
# português) e o cancela; instala em silêncio; confere os arquivos, o versoes.txt, o atalho do Menu
# Iniciar (destino, pasta, AppUserModelID), o registro e as propriedades do atalho; abre o Vitral
# pelo atalho (o lançador abre o app-<versão>\vitral.exe e sai); confere a escolha do lançador no
# Windows (atual, anterior, maior pasta, com os argumentos e o VITRAL_LANCADOR, por um vitral.exe de
# mentira que anota o que recebeu); abre o zip portátil direto e o lançador dele (que só explica);
# deixa os restos que o atualizador pode deixar e desinstala em silêncio, conferindo o que sobrou
# (os dados e a configuração ficam). O que acontece vai para <pasta_dos_prints>/instalador.log, as
# conferências para conferencias.txt e os logs do Inno para instalar.log e desinstalar.log. Vai até o
# fim e sai com 1 se alguma conferência falhou.
param(
    [Parameter(Mandatory)] [string] $Pacote,
    [Parameter(Mandatory)] [string] $Saida
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Saida | Out-Null
$Saida = (Resolve-Path $Saida).Path
$Pacote = (Resolve-Path $Pacote).Path
$log = Join-Path $Saida 'instalador.log'
$conferencias = Join-Path $Saida 'conferencias.txt'
$falhas = [System.Collections.Generic.List[string]]::new()
function Anotar([string] $texto) {
    $linha = "[{0:HH:mm:ss}] {1}" -f (Get-Date), $texto
    Write-Host $linha
    Add-Content -Path $log -Value $linha -Encoding utf8
}
function Conferir([bool] $certo, [string] $texto) {
    $linha = if ($certo) { "ok      $texto" } else { "FALHOU  $texto" }
    if (-not $certo) { $falhas.Add($texto) }
    Anotar $linha
    Add-Content -Path $conferencias -Value $linha -Encoding utf8
}

Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Tela {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Ansi)]
    public struct DEVMODE {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
        public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra;
        public int dmFields, dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
        public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
        public short dmLogPixels;
        public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
        public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2;
        public int dmPanningWidth, dmPanningHeight;
    }
    [DllImport("user32.dll")] public static extern bool EnumDisplaySettings(string nome, int modo, ref DEVMODE d);
    [DllImport("user32.dll")] public static extern int ChangeDisplaySettings(ref DEVMODE d, int opcoes);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern void keybd_event(byte tecla, byte scan, uint f, UIntPtr e);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool SystemParametersInfo(uint acao, uint p, string valor, uint opcoes);

    public static int Resolucao(int largura, int altura) {
        var d = new DEVMODE();
        d.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
        if (!EnumDisplaySettings(null, -1, ref d)) return -100;
        d.dmPelsWidth = largura; d.dmPelsHeight = altura;
        d.dmFields = 0x80000 | 0x100000;
        return ChangeDisplaySettings(ref d, 0);
    }
    public static void Tecla(byte tecla) {
        keybd_event(tecla, 0, 0, UIntPtr.Zero);
        keybd_event(tecla, 0, 2, UIntPtr.Zero);
    }

    // O System.AppUserModel.ID de um atalho (ou de uma janela) pelo IPropertyStore do Shell: é o que a
    // barra de tarefas e as notificações leem.
    [StructLayout(LayoutKind.Sequential, Pack = 4)] public struct PropertyKey { public Guid fmtid; public uint pid; }
    // O PROPVARIANT tem 24 bytes no x64: o tipo, três palavras reservadas e a união.
    [StructLayout(LayoutKind.Explicit)] public struct PropVariant {
        [FieldOffset(0)] public ushort vt;
        [FieldOffset(8)] public IntPtr p;
        [FieldOffset(16)] public IntPtr p2;
    }
    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IPropertyStore {
        void GetCount(out uint n);
        void GetAt(uint i, out PropertyKey k);
        void GetValue(ref PropertyKey k, out PropVariant v);
        void SetValue(ref PropertyKey k, ref PropVariant v);
        void Commit();
    }
    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = false)]
    static extern void SHGetPropertyStoreFromParsingName(string caminho, IntPtr bc, int flags, ref Guid iid, [MarshalAs(UnmanagedType.Interface)] out IPropertyStore loja);
    [DllImport("shell32.dll", PreserveSig = false)]
    static extern void SHGetPropertyStoreForWindow(IntPtr janela, ref Guid iid, [MarshalAs(UnmanagedType.Interface)] out IPropertyStore loja);
    [DllImport("ole32.dll")] static extern int PropVariantClear(ref PropVariant v);

    static string Aumid(IPropertyStore loja) {
        var chave = new PropertyKey { fmtid = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), pid = 5 };
        PropVariant v;
        loja.GetValue(ref chave, out v);
        try {
            if (v.vt == 31) return Marshal.PtrToStringUni(v.p);
            return v.vt == 0 ? "" : "(vt " + v.vt + ")";
        } finally { PropVariantClear(ref v); }
    }
    public static string AumidDoArquivo(string caminho) {
        var iid = new Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99");
        IPropertyStore loja;
        SHGetPropertyStoreFromParsingName(caminho, IntPtr.Zero, 0, ref iid, out loja);
        return Aumid(loja);
    }
    public static string AumidDaJanela(IntPtr janela) {
        var iid = new Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99");
        IPropertyStore loja;
        SHGetPropertyStoreForWindow(janela, ref iid, out loja);
        return Aumid(loja);
    }
}
'@

function Print([string] $nome) {
    $tela = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $imagem = New-Object System.Drawing.Bitmap $tela.Width, $tela.Height
    $g = [System.Drawing.Graphics]::FromImage($imagem)
    $g.CopyFromScreen($tela.Location, [System.Drawing.Point]::Empty, $tela.Size)
    $imagem.Save((Join-Path $Saida "$nome.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $imagem.Dispose()
    Anotar "print $nome"
}
function Soma([string] $caminho) { (Get-FileHash -Algorithm SHA256 $caminho).Hash }
# Os mesmos nomes, sem olhar a ordem (o Sort-Object do PowerShell segue o idioma: o hífen quase não
# conta), com as maiúsculas.
function MesmosNomes($a, $b) { ((@($a) | Sort-Object) -join '|') -ceq ((@($b) | Sort-Object) -join '|') }
# Os processos de um executável, pelo caminho (o lançador é Vitral.exe e o app vitral.exe: o nome
# não separa os dois).
function Processos([string] $exe) {
    @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and ($_.ExecutablePath -ieq $exe) })
}
# Espera a janela principal de algum processo de `exe` (até `segundos`); devolve o processo ou $null.
function EsperarJanela([string] $exe, [int] $segundos) {
    foreach ($i in 1..$segundos) {
        foreach ($cim in Processos $exe) {
            $p = Get-Process -Id $cim.ProcessId -ErrorAction SilentlyContinue
            if ($p -and $p.MainWindowHandle -ne [IntPtr]::Zero) { return $p }
        }
        Start-Sleep -Seconds 1
    }
    return $null
}
# Fecha pela janela (WM_CLOSE, como o ×) e, se não sair em 20 s, à força.
function Fechar($p, [string] $nome) {
    if (-not $p) { return }
    [void][Tela]::PostMessage($p.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
    if (-not $p.WaitForExit(20000)) {
        Stop-Process -Id $p.Id -Force
        Anotar "$nome não fechou pelo ×: parado à força"
    } else {
        Anotar "$nome fechou pelo ×"
    }
}
function Listar([string] $pasta) {
    if (-not (Test-Path $pasta)) { Anotar "  ($pasta não existe)"; return }
    Get-ChildItem $pasta -Recurse -Force | ForEach-Object {
        $relativo = $_.FullName.Substring($pasta.Length).TrimStart('\')
        if ($_.PSIsContainer) { Anotar "  $relativo\" } else { Anotar ("  {0}  ({1} bytes)" -f $relativo, $_.Length) }
    }
}

Anotar ("resolução 1920x1080: ChangeDisplaySettings = {0}" -f [Tela]::Resolucao(1920, 1080))
Anotar ("Windows {0}, PowerShell {1}" -f [Environment]::OSVersion.VersionString, $PSVersionTable.PSVersion)
# Um papel de parede com cor, como nos prints do app (o acrílico desfoca o que está atrás).
$papel = Get-ChildItem 'C:\Windows\Web\Wallpaper' -Recurse -Include *.jpg -ErrorAction SilentlyContinue | Select-Object -First 1
if ($papel) { [void][Tela]::SystemParametersInfo(0x0014, 0, $papel.FullName, 3) }

# --- Os pacotes -------------------------------------------------------------------------------
$instalador = Get-ChildItem $Pacote -Filter 'vitral-*-windows-x86_64-instalador.exe' | Select-Object -First 1
$zip = Get-ChildItem $Pacote -Filter 'vitral-*-windows-x86_64.zip' | Select-Object -First 1
$tgz = Get-ChildItem $Pacote -Filter 'vitral-*-windows-x86_64.tar.gz' | Select-Object -First 1
Conferir ($instalador -and $zip -and $tgz) "os três pacotes do Windows: $($instalador.Name), $($zip.Name), $($tgz.Name)"
$versao = $instalador.Name -replace '^vitral-(.+)-windows-x86_64-instalador\.exe$', '$1'
$prefixo = "vitral-$versao-windows-x86_64"
Anotar "versão $versao"
foreach ($f in @($instalador, $zip, $tgz)) { Anotar ("  {0}: {1:N1} MB, SHA-256 {2}" -f $f.Name, ($f.Length / 1MB), (Soma $f.FullName)) }
$info = $instalador.VersionInfo
Anotar ("instalador: ProductName '{0}', ProductVersion '{1}', FileVersion '{2}', FileDescription '{3}'" -f $info.ProductName, $info.ProductVersion, $info.FileVersion, $info.FileDescription)

$trabalho = Join-Path $env:RUNNER_TEMP 'vitral-pacote'
$doZip = Join-Path $trabalho 'zip'
$doTgz = Join-Path $trabalho 'tgz'
New-Item -ItemType Directory -Force $doZip, $doTgz | Out-Null
Expand-Archive -Path $zip.FullName -DestinationPath $doZip
$esperados = @('LEIA-ME.md', 'LICENSE', 'NOVIDADES.md', 'OFL-fontes.txt', 'vitral-lancador.exe', 'vitral.exe')
$noZip = @(Get-ChildItem $doZip -Name | Sort-Object)
Conferir (MesmosNomes $noZip $esperados) "o zip tem na raiz: $($noZip -join ', ')"
$entradas = @(& tar -tzf $tgz.FullName)
Anotar "o .tar.gz: $($entradas -join ', ')"
Conferir (@($entradas | Where-Object { $_ -ne "$prefixo/" -and -not $_.StartsWith("$prefixo/") }).Count -eq 0) "o .tar.gz só tem a pasta $prefixo/"
& tar -xzf $tgz.FullName -C $doTgz
$iguais = $true
foreach ($nome in $esperados) {
    $a = Join-Path $doZip $nome; $b = Join-Path (Join-Path $doTgz $prefixo) $nome
    if (-not (Test-Path $b) -or (Soma $a) -ne (Soma $b)) { $iguais = $false; Anotar "  diferente no .tar.gz: $nome" }
}
Conferir $iguais 'o .tar.gz tem o mesmo conteúdo do zip'
$lancadorDoPacote = Join-Path $doZip 'vitral-lancador.exe'
$vi = (Get-Item $lancadorDoPacote).VersionInfo
Anotar ("lançador: FileDescription '{0}', OriginalFilename '{1}', FileVersion '{2}', ProductName '{3}'" -f $vi.FileDescription, $vi.OriginalFilename, $vi.FileVersion, $vi.ProductName)
Conferir ($vi.FileDescription -ceq 'Vitral' -and $vi.OriginalFilename -ceq 'Vitral.exe') 'o lançador tem a versão embutida (Vitral, Vitral.exe)'
# Os ícones que o Explorer mostra (o lançador sem o ícone embutido sairia com o genérico).
foreach ($par in @(@('icone-lancador', $lancadorDoPacote), @('icone-vitral', (Join-Path $doZip 'vitral.exe')))) {
    $icone = [System.Drawing.Icon]::ExtractAssociatedIcon($par[1])
    $icone.ToBitmap().Save((Join-Path $Saida "$($par[0]).png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $icone.Dispose()
}
Conferir ((Soma (Join-Path $Saida 'icone-lancador.png')) -eq (Soma (Join-Path $Saida 'icone-vitral.png'))) 'o lançador tem o mesmo ícone do vitral.exe'

# --- O instalador aberto, em português, e cancelado --------------------------------------------
$raiz = Join-Path $env:LOCALAPPDATA 'Programs\Vitral'
$dados = Join-Path $env:LOCALAPPDATA 'vitral'
$config = Join-Path $env:APPDATA 'vitral'
Conferir (-not (Test-Path $raiz) -and -not (Test-Path $dados) -and -not (Test-Path $config)) 'antes: sem instalação, sem dados e sem configuração'
$assistente = Start-Process -FilePath $instalador.FullName -PassThru
Start-Sleep -Seconds 8
Print '01-instalador-aberto'
# O assistente roda num .tmp que o instalador abre: para os dois.
Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'vitral-*-instalador*' } | ForEach-Object {
    Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
}
[void]$assistente.WaitForExit(10000)
Start-Sleep -Seconds 2
Conferir (-not (Test-Path (Join-Path $raiz 'Vitral.exe'))) 'o instalador cancelado não instalou nada'

# --- Instalar em silêncio --------------------------------------------------------------------------
$p = Start-Process -FilePath $instalador.FullName -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$(Join-Path $Saida 'instalar.log')`"") -Wait -PassThru
Conferir ($p.ExitCode -eq 0) "instalação em silêncio: código $($p.ExitCode)"
Anotar "a pasta $raiz"
Listar $raiz
$lancador = Join-Path $raiz 'Vitral.exe'
$app = Join-Path $raiz "app-$versao"
Conferir (Test-Path $lancador) 'Vitral.exe (o lançador) na raiz'
Conferir ((Soma $lancador) -eq (Soma $lancadorDoPacote)) 'o Vitral.exe da raiz é o vitral-lancador.exe do pacote'
$naApp = @(Get-ChildItem $app -Name -ErrorAction SilentlyContinue | Sort-Object)
Conferir (MesmosNomes $naApp $esperados) "app-$versao\ tem o pacote inteiro: $($naApp -join ', ')"
Conferir ((Soma (Join-Path $app 'vitral.exe')) -eq (Soma (Join-Path $doZip 'vitral.exe'))) "app-$versao\vitral.exe é o do pacote"
$versoes = Get-Content (Join-Path $raiz 'versoes.txt') -Raw
Anotar ("versoes.txt: {0}" -f ($versoes -replace "`r", '\r' -replace "`n", '\n'))
Conferir ($versoes -ceq "atual=$versao`r`n") 'versoes.txt aponta para a versão instalada (sem anterior)'
Conferir (Test-Path (Join-Path $raiz 'unins000.exe')) 'o desinstalador na raiz'

# O atalho do Menu Iniciar (o do usuário: instalação sem administrador) e o da área de trabalho (desmarcado).
$atalho = Join-Path ([Environment]::GetFolderPath('Programs')) 'Vitral.lnk'
Conferir (Test-Path $atalho) "atalho em $atalho"
$ws = New-Object -ComObject WScript.Shell
$lnk = $ws.CreateShortcut($atalho)
Anotar ("atalho: destino '{0}', argumentos '{1}', iniciar em '{2}', ícone '{3}'" -f $lnk.TargetPath, $lnk.Arguments, $lnk.WorkingDirectory, $lnk.IconLocation)
Conferir ($lnk.TargetPath -ieq $lancador) 'o atalho aponta para o lançador (nunca para a app-<versão>)'
Conferir ($lnk.WorkingDirectory -ieq $env:USERPROFILE) 'o atalho abre na pasta do usuário'
$aumid = try { [Tela]::AumidDoArquivo($atalho) } catch { "erro: $_" }
Conferir ($aumid -ceq 'VictorMenegate.Vitral') "AppUserModelID do atalho: '$aumid'"
Conferir (-not (Test-Path (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Vitral.lnk'))) 'sem atalho na área de trabalho (a tarefa vem desmarcada)'
# O que o Menu Iniciar indexou: um app com AppUserModelID aparece com ele, não com o caminho.
foreach ($i in 1..10) {
    $apps = powershell.exe -NoProfile -Command "Get-StartApps | Where-Object Name -like '*Vitral*' | ForEach-Object { `$_.Name + ' | ' + `$_.AppID }" 2>&1
    if ($apps) { break }
    Start-Sleep -Seconds 2
}
Anotar "Get-StartApps: $($apps -join '; ')"
if ("$apps" -match 'Get-StartApps') {
    Anotar 'sem o Get-StartApps neste Windows: fica só a conferência do atalho'
} else {
    Conferir (@($apps | Where-Object { $_ -ceq 'Vitral | VictorMenegate.Vitral' }).Count -eq 1) 'o Menu Iniciar conhece o Vitral pelo AppUserModelID'
}

$chave = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{DE776FCA-34A5-4442-A784-DC2E5BDEC95F}_is1'
$desinstalar = Get-ItemProperty $chave -ErrorAction SilentlyContinue
Anotar ("Aplicativos instalados: '{0}' {1}, de '{2}', em '{3}', ícone '{4}'" -f $desinstalar.DisplayName, $desinstalar.DisplayVersion, $desinstalar.Publisher, $desinstalar.InstallLocation, $desinstalar.DisplayIcon)
Conferir ($desinstalar.DisplayName -ceq 'Vitral' -and $desinstalar.DisplayVersion -ceq $versao) 'em Aplicativos instalados como Vitral, com a versão'
Conferir ("$($desinstalar.InstallLocation)".TrimEnd('\') -ieq $raiz) 'instalado na pasta do usuário, sem administrador'
$notificacoes = Get-ItemProperty 'HKCU:\Software\Classes\AppUserModelId\VictorMenegate.Vitral' -ErrorAction SilentlyContinue
Conferir ($notificacoes.DisplayName -ceq 'Vitral') 'o nome das notificações registrado (AppUserModelId\VictorMenegate.Vitral)'

# As propriedades do atalho, como a pessoa conferiria (num processo à parte, que mantém a janela).
$propriedades = Start-Process -FilePath powershell.exe -PassThru -ArgumentList @('-NoProfile', '-Command',
    "(New-Object -ComObject Shell.Application).Namespace('$(Split-Path $atalho)').ParseName('Vitral.lnk').InvokeVerb('properties'); Start-Sleep -Seconds 25")
Start-Sleep -Seconds 6
Print '02-propriedades-do-atalho'
Stop-Process -Id $propriedades.Id -Force -ErrorAction SilentlyContinue
# O Menu Iniciar com o Vitral (a busca pode não existir no Windows Server; o print mostra o que der).
[Tela]::Tecla(0x5B)
Start-Sleep -Seconds 4
[System.Windows.Forms.SendKeys]::SendWait('Vitral')
Start-Sleep -Seconds 4
Print '03-menu-iniciar'
[Tela]::Tecla(0x1B)
Start-Sleep -Seconds 2
Start-Process -FilePath explorer.exe -ArgumentList "`"$raiz`""
Start-Sleep -Seconds 5
Print '04-pasta-instalada'
(New-Object -ComObject Shell.Application).Windows() | ForEach-Object { try { $_.Quit() } catch {} }

# --- Abrir pelo atalho: o lançador abre a versão e sai -------------------------------------------
$exeApp = Join-Path $app 'vitral.exe'
Start-Process -FilePath $atalho
$vitral = EsperarJanela $exeApp 90
Conferir ($null -ne $vitral) "o atalho abriu o $exeApp com janela"
if ($vitral) {
    $cim = Get-CimInstance Win32_Process -Filter "ProcessId = $($vitral.Id)"
    Anotar ("vitral.exe: pid {0}, linha de comando {1}, pai {2}" -f $vitral.Id, $cim.CommandLine, $cim.ParentProcessId)
    Start-Sleep -Seconds 3
    Conferir ((Processos $lancador).Count -eq 0) 'o lançador saiu sem esperar o app'
    Start-Sleep -Seconds 20
    [void][Tela]::SetForegroundWindow($vitral.MainWindowHandle)
    Start-Sleep -Seconds 2
    Print '05-aberto-pelo-atalho'
    try { Anotar ("AppUserModelID da janela: '{0}' (o do processo é o set_app_identity, que não aparece aqui)" -f [Tela]::AumidDaJanela($vitral.MainWindowHandle)) } catch { Anotar "AppUserModelID da janela: $_" }
    Fechar $vitral 'o Vitral instalado'
}
Conferir (Test-Path $dados) "o Vitral criou os dados em $dados"
# A configuração só aparece quando algo muda (o zoom, as animações): a de mentira faz as vezes dela.
if (-not (Test-Path $config)) {
    Anotar "sem $config ainda: uma configuração de mentira para conferir que ela fica"
    New-Item -ItemType Directory -Force $config | Out-Null
    Set-Content -Path (Join-Path $config 'interface.json') -Value '{"zoom": 1.0}'
}

# --- A escolha do lançador no Windows ----------------------------------------------------------------
# Um vitral.exe de mentira (C#, compilado pelo csc do .NET Framework) que anota onde está, a pasta,
# o VITRAL_LANCADOR e os argumentos em VITRAL_ECO, e sai.
$eco = Join-Path $trabalho 'eco.cs'
Set-Content -Path $eco -Encoding utf8 -Value @'
using System; using System.IO; using System.Linq;
class Eco {
    static void Main(string[] argumentos) {
        var linhas = new[] {
            "exe=" + System.Reflection.Assembly.GetEntryAssembly().Location,
            "pasta=" + Environment.CurrentDirectory,
            "lancador=" + Environment.GetEnvironmentVariable("VITRAL_LANCADOR"),
        }.Concat(argumentos.Select(a => "arg=" + a));
        File.WriteAllLines(Environment.GetEnvironmentVariable("VITRAL_ECO"), linhas);
    }
}
'@
$csc = Join-Path $env:WINDIR 'Microsoft.NET\Framework64\v4.0.30319\csc.exe'
foreach ($v in @('9.9.9', '0.0.1')) {
    New-Item -ItemType Directory -Force (Join-Path $raiz "app-$v") | Out-Null
    & $csc /nologo /target:winexe "/out:$(Join-Path $raiz "app-$v\vitral.exe")" $eco | ForEach-Object { Anotar "csc: $_" }
}
$argumentos = @('C:\Projetos\loja com espaço', '--nova', 'loja', 'o que "fazer" agora')
$casos = @(
    @('atual', "atual=0.0.1`r`nanterior=9.9.9`r`n", '0.0.1'),
    @('anterior', "atual=0.0.2`r`nanterior=0.0.1`r`n", '0.0.1'),
    @('maior pasta', "atual=0.0.2`r`nanterior=0.0.3`r`n", '9.9.9'),
    @('sem versoes.txt', $null, '9.9.9')
)
foreach ($caso in $casos) {
    $arquivo = Join-Path $raiz 'versoes.txt'
    if ($null -eq $caso[1]) { Remove-Item $arquivo -Force -ErrorAction SilentlyContinue } else { [IO.File]::WriteAllText($arquivo, $caso[1]) }
    $env:VITRAL_ECO = Join-Path $trabalho "eco-$($caso[0] -replace ' ', '-').txt"
    Push-Location $env:USERPROFILE
    & $lancador @argumentos
    Pop-Location
    foreach ($i in 1..30) { if (Test-Path $env:VITRAL_ECO) { break }; Start-Sleep -Milliseconds 500 }
    $anotado = @(Get-Content $env:VITRAL_ECO -ErrorAction SilentlyContinue)
    Anotar ("lançador, {0}: {1}" -f $caso[0], ($anotado -join ' | '))
    # Os caminhos sem diferença de maiúsculas (o Windows não as diferencia); os argumentos, exatos.
    $caminhos = @("exe=$(Join-Path $raiz "app-$($caso[2])\vitral.exe")", "pasta=$env:USERPROFILE", "lancador=$lancador")
    $certo = $anotado.Count -eq 3 + $argumentos.Count
    for ($i = 0; $certo -and $i -lt $anotado.Count; $i++) {
        $certo = if ($i -lt 3) { $anotado[$i] -ieq $caminhos[$i] } else { $anotado[$i] -ceq "arg=$($argumentos[$i - 3])" }
    }
    Conferir $certo "lançador, $($caso[0]): abre a app-$($caso[2]) com os mesmos argumentos, a mesma pasta e o VITRAL_LANCADOR"
}
Remove-Item Env:VITRAL_ECO
[IO.File]::WriteAllText((Join-Path $raiz 'versoes.txt'), "atual=$versao`r`n")

# --- O zip portátil, aberto direto ---------------------------------------------------------------
$portatil = Join-Path $trabalho 'portatil'
Expand-Archive -Path $zip.FullName -DestinationPath $portatil
$exePortatil = Join-Path $portatil 'vitral.exe'
Start-Process -FilePath $exePortatil -WorkingDirectory $portatil
$vitral = EsperarJanela $exePortatil 90
Conferir ($null -ne $vitral) 'o vitral.exe do zip abriu direto, com janela'
if ($vitral) {
    Start-Sleep -Seconds 20
    [void][Tela]::SetForegroundWindow($vitral.MainWindowHandle)
    Start-Sleep -Seconds 2
    Print '06-portatil'
    Fechar $vitral 'o Vitral portátil'
}
# O lançador do zip não tem versoes.txt nem app-*: só explica (a caixa de mensagem).
$exeLancadorPortatil = Join-Path $portatil 'vitral-lancador.exe'
Start-Process -FilePath $exeLancadorPortatil
$caixa = EsperarJanela $exeLancadorPortatil 20
Conferir ($null -ne $caixa) "o lançador fora da instalação mostra a caixa de mensagem ('$($caixa.MainWindowTitle)')"
Start-Sleep -Seconds 2
Print '07-lancador-do-zip'
if ($caixa) { Stop-Process -Id $caixa.Id -Force }

# --- Desinstalar em silêncio ---------------------------------------------------------------------
# O que o atualizador (W4) pode deixar na raiz, e um arquivo que não é do Vitral (fica).
New-Item -ItemType Directory -Force (Join-Path $raiz '.baixando'), (Join-Path $raiz ".novo-9.9.9-1234") | Out-Null
foreach ($resto in @('.baixando\vitral-9.9.9-windows-x86_64.tar.gz', ".novo-9.9.9-1234\vitral.exe", 'estado.json', '.estado.json.1234.tmp', 'versoes.txt.novo', 'Vitral.exe.novo', '.trava')) {
    Set-Content -Path (Join-Path $raiz $resto) -Value 'resto'
}
Set-Content -Path (Join-Path $raiz 'anotacao-da-pessoa.txt') -Value 'não é do Vitral'
Anotar 'antes de desinstalar:'
Listar $raiz
$desinstalador = Join-Path $raiz 'unins000.exe'
$p = Start-Process -FilePath $desinstalador -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$(Join-Path $Saida 'desinstalar.log')`"") -Wait -PassThru
Anotar "desinstalador: código $($p.ExitCode)"
# Ele se copia para a pasta temporária e termina de lá: espera o unins000.exe sumir.
foreach ($i in 1..120) {
    if (-not (Test-Path $desinstalador) -and -not (Get-CimInstance Win32_Process | Where-Object { $_.Name -like '_iu*' })) { break }
    Start-Sleep -Seconds 1
}
Conferir ($p.ExitCode -eq 0) "desinstalação em silêncio: código $($p.ExitCode)"
Anotar 'depois de desinstalar:'
Listar $raiz
$sobrou = @(Get-ChildItem $raiz -Force -Name -ErrorAction SilentlyContinue)
Conferir (($sobrou -join ',') -ceq 'anotacao-da-pessoa.txt') "da pasta do programa só sobrou o que não é do Vitral: $($sobrou -join ', ')"
Conferir (-not (Test-Path $atalho)) 'o atalho do Menu Iniciar saiu'
Conferir (-not (Test-Path $chave)) 'saiu de Aplicativos instalados'
Conferir (-not (Test-Path 'HKCU:\Software\Classes\AppUserModelId\VictorMenegate.Vitral')) 'o registro das notificações saiu'
Conferir ((Test-Path $dados) -and (Test-Path $config)) 'os dados e a configuração ficaram'
Anotar "os dados ($dados):"
Get-ChildItem $dados -Force -Name | ForEach-Object { Anotar "  $_" }
Anotar "a configuração ($config):"
Get-ChildItem $config -Force -Name | ForEach-Object { Anotar "  $_" }
Start-Process -FilePath explorer.exe -ArgumentList "`"$env:LOCALAPPDATA`""
Start-Sleep -Seconds 5
Print '08-depois-de-desinstalar'
(New-Object -ComObject Shell.Application).Windows() | ForEach-Object { try { $_.Quit() } catch {} }
Remove-Item -Recurse -Force $raiz -ErrorAction SilentlyContinue

if ($falhas.Count -gt 0) {
    Anotar "$($falhas.Count) conferência(s) falharam: $($falhas -join '; ')"
    exit 1
}
Anotar 'fim: tudo conferido'
