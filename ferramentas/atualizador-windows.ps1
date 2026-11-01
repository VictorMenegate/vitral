# O atualizador do Windows num Windows de verdade (o runner windows-2025 do GitHub), sem ninguém
# olhando. Roda no .github/workflows/windows-prints.yml (job do atualizador), que antes compila três
# versões do ramo no canal de teste (VITRAL_CANAL=teste: instala sozinho como o release, mas aceita
# o release falso e a chave de teste), monta o instalador da primeira e os pacotes das outras duas, e
# assina os manifestos com uma chave de teste gerada ali mesmo (ferramentas/release_falso.py).
#
# Uso: ferramentas/atualizador-windows.ps1 <instalador publicado> <instalador do ramo> <pasta do release falso> <pasta_dos_prints>
#
# Na ordem: instala a versão publicada (o instalador do release de verdade) e abre; instala por cima
# a primeira do ramo (A); serve o release falso com a segunda (B) e abre a A pelo lançador: ela
# procura (60 s depois de abrir), baixa e instala a B com o app aberto, e o Reiniciar do aviso abre a
# B pelo lançador (o caminho PowerShell do GPUI › Vitral.exe › app-B\vitral.exe, conferido pelos
# eventos de início de processo). Depois publica a terceira (C), que a B instala do mesmo jeito; a C
# cai antes do primeiro quadro (VITRAL_TESTE_CAI, só no canal de teste): o Reiniciar e mais uma
# abertura pelo lançador caem, e a terceira abertura volta sozinha para a B, com o aviso. Entre a
# B instalada e o Reiniciar, o roteiro tira a última procura do atualizacoes.json (as 6 h entre
# procuras, encurtadas). O que acontece vai para <pasta_dos_prints>/atualizador.log, as conferências
# para conferencias.txt, os pedidos ao release falso para pedidos.log. Vai até o fim e sai com 1 se
# alguma conferência falhou.
param(
    [Parameter(Mandatory)] [string] $Publicado,
    [Parameter(Mandatory)] [string] $Instalador,
    [Parameter(Mandatory)] [string] $Falso,
    [Parameter(Mandatory)] [string] $Saida
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Saida | Out-Null
$Saida = (Resolve-Path $Saida).Path
$Falso = (Resolve-Path $Falso).Path
$log = Join-Path $Saida 'atualizador.log'
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
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
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
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, int d, UIntPtr e);
    [DllImport("user32.dll")] public static extern void keybd_event(byte tecla, byte scan, uint f, UIntPtr e);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
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
    // O GPUI responde ao clique pelo último lugar do ponteiro que ele viu: mexe antes de clicar.
    public static void Clicar(int x, int y) {
        SetCursorPos(x, y - 2);
        System.Threading.Thread.Sleep(150);
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(250);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Rolar(int x, int y, int entalhes) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(150);
        for (int i = 0; i < Math.Abs(entalhes); i++) {
            mouse_event(0x0800, 0, 0, entalhes < 0 ? -120 : 120, UIntPtr.Zero);
            System.Threading.Thread.Sleep(40);
        }
    }
    // Ctrl + uma tecla (VK), para a janela da frente.
    public static void Ctrl(byte tecla) {
        keybd_event(0x11, 0, 0, UIntPtr.Zero);
        keybd_event(tecla, 0, 0, UIntPtr.Zero);
        keybd_event(tecla, 0, 2, UIntPtr.Zero);
        keybd_event(0x11, 0, 2, UIntPtr.Zero);
    }
    public static void Tecla(byte tecla) {
        keybd_event(tecla, 0, 0, UIntPtr.Zero);
        keybd_event(tecla, 0, 2, UIntPtr.Zero);
    }
}
'@

function Captura {
    $tela = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $imagem = New-Object System.Drawing.Bitmap $tela.Width, $tela.Height
    $g = [System.Drawing.Graphics]::FromImage($imagem)
    $g.CopyFromScreen($tela.Location, [System.Drawing.Point]::Empty, $tela.Size)
    $g.Dispose()
    return $imagem
}
function Print([string] $nome) {
    $imagem = Captura
    $imagem.Save((Join-Path $Saida "$nome.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $imagem.Dispose()
    Anotar "print $nome"
}
function Processos([string] $exe) {
    @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and ($_.ExecutablePath -ieq $exe) })
}
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
function Frente($p) {
    [void][Tela]::SetForegroundWindow($p.MainWindowHandle)
    Start-Sleep -Milliseconds 800
}
function Retangulo($p) {
    $r = New-Object Tela+RECT
    [void][Tela]::GetWindowRect($p.MainWindowHandle, [ref] $r)
    return $r
}
# O "Reiniciar" do aviso "Versão X pronta": o texto azul de destaque (#6B9BFF) mais à direita na
# faixa de cima, no meio da janela, onde ficam os avisos. Devolve o centro, ou $null.
function AcharReiniciar($p) {
    $r = Retangulo $p
    $meio = [int](($r.Left + $r.Right) / 2)
    $imagem = Captura
    $pontos = [System.Collections.Generic.List[int[]]]::new()
    for ($y = $r.Top + 8; $y -lt $r.Top + 240; $y++) {
        for ($x = $meio - 210; $x -lt $meio + 210; $x++) {
            $c = $imagem.GetPixel($x, $y)
            if ([Math]::Abs($c.R - 107) -lt 40 -and [Math]::Abs($c.G - 155) -lt 40 -and $c.B -gt 215) { $pontos.Add(@($x, $y)) }
        }
    }
    $imagem.Dispose()
    if ($pontos.Count -lt 12) { Anotar "azul de destaque no alto: $($pontos.Count) pontos"; return $null }
    $maior = ($pontos | ForEach-Object { $_[0] } | Measure-Object -Maximum).Maximum
    $direita = @($pontos | Where-Object { $_[0] -ge $maior - 90 })
    $x = [int](($direita | ForEach-Object { $_[0] } | Measure-Object -Average).Average)
    $y = [int](($direita | ForEach-Object { $_[1] } | Measure-Object -Average).Average)
    Anotar "Reiniciar achado em ($x,$y), $($direita.Count) de $($pontos.Count) pontos"
    return @($x, $y)
}
function Ler([string] $caminho) {
    if (Test-Path $caminho) { return (Get-Content $caminho -Raw) } else { return '' }
}
function Mostrar([string] $texto) { $texto -replace "`r", '\r' -replace "`n", '\n' }
function Esperar([scriptblock] $condicao, [int] $segundos, [string] $oque) {
    foreach ($i in 1..$segundos) {
        if (& $condicao) { Anotar "$oque (depois de $i s)"; return $true }
        Start-Sleep -Seconds 1
    }
    Anotar "${oque}: não aconteceu em $segundos s"
    return $false
}

Anotar ("resolução 1920x1080: ChangeDisplaySettings = {0}" -f [Tela]::Resolucao(1920, 1080))
$papel = Get-ChildItem 'C:\Windows\Web\Wallpaper' -Recurse -Include *.jpg -ErrorAction SilentlyContinue | Select-Object -First 1
if ($papel) { [void][Tela]::SystemParametersInfo(0x0014, 0, $papel.FullName, 3) }

$raiz = Join-Path $env:LOCALAPPDATA 'Programs\Vitral'
$lancador = Join-Path $raiz 'Vitral.exe'
$versoes = Join-Path $raiz 'versoes.txt'
$estado = Join-Path $raiz 'estado.json'
$config = Join-Path $env:APPDATA 'vitral\atualizacoes.json'
$v6 = (Split-Path $Publicado -Leaf) -replace '^vitral-(.+)-windows-x86_64-instalador\.exe$', '$1'
$vA = (Split-Path $Instalador -Leaf) -replace '^vitral-(.+)-windows-x86_64-instalador\.exe$', '$1'
$vB = (Get-Content (Join-Path $Falso 'rB\vitral-versao.json') -Raw | ConvertFrom-Json).versao
$vC = (Get-Content (Join-Path $Falso 'rC\vitral-versao.json') -Raw | ConvertFrom-Json).versao
Anotar "publicada $v6; do ramo: A $vA (instalador), B $vB e C $vC (release falso)"
function App([string] $v) { Join-Path $raiz "app-$v\vitral.exe" }

# Os inícios de processo (com o pai), para ver o caminho do Reiniciar: GPUI › PowerShell › lançador › app.
$inicios = [System.Collections.Generic.List[object]]::new()
try {
    Register-CimIndicationEvent -ClassName Win32_ProcessStartTrace -SourceIdentifier inicios -ErrorAction Stop
    $comEventos = $true
} catch {
    Anotar "sem os eventos de início de processo: $_"
    $comEventos = $false
}
function Inicios {
    if (-not $comEventos) { return }
    foreach ($e in @(Get-Event -SourceIdentifier inicios -ErrorAction SilentlyContinue)) {
        $n = $e.SourceEventArgs.NewEvent
        $inicios.Add([pscustomobject]@{ Pid = [int]$n.ProcessID; Pai = [int]$n.ParentProcessID; Nome = $n.ProcessName })
        Remove-Event -EventIdentifier $e.EventIdentifier
    }
}
# A cadeia de pais de `pid` pelos inícios vistos: "app ‹ lançador ‹ powershell ‹ ...".
function Cadeia([int] $pid_) {
    Inicios
    $nomes = @()
    $atual = $pid_
    for ($i = 0; $i -lt 5; $i++) {
        $e = $inicios | Where-Object { $_.Pid -eq $atual } | Select-Object -Last 1
        if (-not $e) { break }
        $nomes += $e.Nome
        $atual = $e.Pai
    }
    return $nomes
}

# --- 1. A versão publicada, pelo instalador do release de verdade ----------------------------------
$p = Start-Process -FilePath $Publicado -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$(Join-Path $Saida 'instalar-publicada.log')`"") -Wait -PassThru
Conferir ($p.ExitCode -eq 0) "a $v6 publicada instalou em silêncio"
Conferir ((Ler $versoes) -ceq "atual=$v6`r`n") "versoes.txt: $(Mostrar (Ler $versoes))"
Start-Process -FilePath $lancador
$vitral = EsperarJanela (App $v6) 90
Conferir ($null -ne $vitral) "a $v6 abriu pelo lançador"
if ($vitral) {
    Start-Sleep -Seconds 20
    Frente $vitral
    Print '01-publicada-aberta'
    Fechar $vitral "a $v6"
}

# --- 2. A A do ramo por cima, pelo instalador -----------------------------------------------------
$p = Start-Process -FilePath $Instalador -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$(Join-Path $Saida 'instalar-a.log')`"") -Wait -PassThru
Conferir ($p.ExitCode -eq 0) "a $vA do ramo instalou por cima"
Conferir ((Ler $versoes) -ceq "atual=$vA`r`nanterior=$v6`r`n") "versoes.txt: $(Mostrar (Ler $versoes))"

# --- 3. O release falso com a B, e a A aberta pelo lançador ----------------------------------------
$servida = Join-Path $Falso 'r'
New-Item -ItemType Directory -Force $servida | Out-Null
Copy-Item (Join-Path $Falso 'rB\*') $servida
$servidor = Start-Process -FilePath python -ArgumentList @('-I', 'ferramentas/release_falso.py', 'servir', $Falso, '18731', '--direto') -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 2
$env:VITRAL_ATUALIZACAO_BASE = 'http://127.0.0.1:18731/r'
$env:VITRAL_ATUALIZACAO_CHAVE = Join-Path $Falso 'bancada.pub'
$env:VITRAL_TESTE_CAI = $vC
Start-Process -FilePath $lancador
$vitral = EsperarJanela (App $vA) 90
Conferir ($null -ne $vitral) "a $vA abriu pelo lançador, com o release falso"
$aberta = Get-Date
Start-Sleep -Seconds 20
Frente $vitral
Print '02-a-aberta'
# As Configurações abertas e roladas até as Atualizações (o progresso do download só anda com elas).
Start-Sleep -Seconds 15
Frente $vitral
[Tela]::Ctrl(0xBC)
Start-Sleep -Seconds 4
$r = Retangulo $vitral
[Tela]::Rolar([int](($r.Left + $r.Right) / 2), [int](($r.Top + $r.Bottom) / 2), -40)
Start-Sleep -Seconds 3
Print '03-configuracoes-antes-de-procurar'
$pedidos = Join-Path $Falso 'pedidos.log'
$procurou = Esperar { (Ler $pedidos) -match 'vitral-versao\.json' } 80 "a $vA pediu o manifesto"
Conferir $procurou ("a $vA procurou sozinha, {0} s depois de abrir" -f [int]((Get-Date) - $aberta).TotalSeconds)
Start-Sleep -Seconds 2
Print '04-procurando'
$baixando = Esperar { @(Get-ChildItem (Join-Path $raiz '.baixando') -Filter *.parte -ErrorAction SilentlyContinue).Count -gt 0 } 30 'o download começou'
Start-Sleep -Seconds 6
Print '05-baixando'
$pronta = Esperar { (Ler $versoes) -like "atual=$vB*" } 90 "a $vB instalada"
Conferir $pronta "a $vA baixou e instalou a $vB"
Conferir ($null -ne (Get-Process -Id $vitral.Id -ErrorAction SilentlyContinue)) "com a $vA aberta o tempo todo"
Conferir ((Ler $versoes) -ceq "atual=$vB`r`nanterior=$vA`r`n") "versoes.txt: $(Mostrar (Ler $versoes))"
Conferir (-not (Test-Path (Join-Path $raiz "app-$v6"))) "a $v6 (nem atual, nem anterior, nem a que roda) saiu"
Anotar "pasta: $((Get-ChildItem $raiz -Force -Name) -join ', ')"
Anotar "estado.json: $(Mostrar (Ler $estado))"
Anotar "pedidos: $(Mostrar (Ler $pedidos))"
Start-Sleep -Seconds 4
Print '06-pronta-nas-configuracoes'
[Tela]::Tecla(0x1B)
Start-Sleep -Seconds 3
Print '07-aviso-pronta'

# --- 4. Reiniciar: o lançador abre a B ------------------------------------------------------------
# As 6 h entre procuras, encurtadas: a B procura 60 s depois de abrir. E o release passa a ter a C.
$cfg = Get-Content $config -Raw | ConvertFrom-Json
$cfg.PSObject.Properties.Remove('ultima_procura')
$cfg | ConvertTo-Json -Depth 5 | Set-Content $config -Encoding utf8
Copy-Item (Join-Path $Falso 'rC\*') $servida -Force
Inicios
Frente $vitral
$alvo = AcharReiniciar $vitral
Conferir ($null -ne $alvo) 'o Reiniciar do aviso está na tela'
if ($alvo) { Frente $vitral; [Tela]::Clicar($alvo[0], $alvo[1]) }
$saiu = $vitral.WaitForExit(30000)
Conferir $saiu "a $vA fechou no Reiniciar"
$vitral = EsperarJanela (App $vB) 90
Conferir ($null -ne $vitral) "o Reiniciar abriu a $vB"
if ($vitral) {
    $cadeia = Cadeia $vitral.Id
    Anotar "quem abriu a ${vB}: $($cadeia -join ' ‹ ')"
    if ($comEventos) {
        # O nome com as maiúsculas da imagem: Vitral.exe é o lançador, vitral.exe o app.
        Conferir ($cadeia.Count -ge 3 -and $cadeia[1] -ceq 'Vitral.exe' -and $cadeia[2] -match 'powershell|pwsh') "a $vB veio do lançador, que veio do PowerShell do Reiniciar do GPUI"
    }
    Start-Sleep -Seconds 6
    Frente $vitral
    Print '08-reiniciou-na-b'
    Start-Sleep -Seconds 12
    Print '09-b-aberta'
}
$confirmada = Esperar { (Ler $estado) -notmatch '"confirmar"' } 50 "a $vB confirmada"
Conferir $confirmada "a $vB ficou confirmada (10 s depois do primeiro quadro): $(Mostrar (Ler $estado))"

# --- 5. A B instala a C, que cai: a terceira abertura volta para a B -------------------------------
$prontaC = Esperar { (Ler $versoes) -like "atual=$vC*" } 150 "a $vC instalada pela $vB"
Conferir $prontaC "a $vB procurou, baixou e instalou a $vC"
Conferir ((Ler $versoes) -ceq "atual=$vC`r`nanterior=$vB`r`n") "versoes.txt: $(Mostrar (Ler $versoes))"
Conferir (-not (Test-Path (Join-Path $raiz "app-$vA"))) "a $vA saiu"
Start-Sleep -Seconds 4
Frente $vitral
Print '10-c-pronta'
Inicios
$alvo = AcharReiniciar $vitral
Conferir ($null -ne $alvo) 'o Reiniciar do aviso da C está na tela'
if ($alvo) { Frente $vitral; [Tela]::Clicar($alvo[0], $alvo[1]) }
[void]$vitral.WaitForExit(30000)
$caiu1 = Esperar { (Ler $estado) -match '"aberturas"' -and @((Ler $estado | ConvertFrom-Json).confirmar.aberturas).Count -ge 1 } 60 "a $vC contou a primeira abertura"
Start-Sleep -Seconds 5
Conferir ((Processos (App $vC)).Count -eq 0) "a $vC caiu antes do primeiro quadro (Reiniciar)"
Anotar "estado.json: $(Mostrar (Ler $estado))"
Start-Process -FilePath $lancador
$caiu2 = Esperar { @((Ler $estado | ConvertFrom-Json).confirmar.aberturas).Count -ge 2 } 60 "a $vC contou a segunda abertura"
Start-Sleep -Seconds 5
Conferir ($caiu1 -and $caiu2 -and (Processos (App $vC)).Count -eq 0) "a $vC caiu de novo (pelo lançador)"
Print '11-c-caiu'
Anotar "estado.json: $(Mostrar (Ler $estado))"
Start-Process -FilePath $lancador
$vitral = EsperarJanela (App $vB) 90
Conferir ($null -ne $vitral) "a terceira abertura voltou sozinha para a $vB"
Conferir ((Ler $versoes) -ceq "atual=$vB`r`n") "versoes.txt: $(Mostrar (Ler $versoes))"
$final = Ler $estado | ConvertFrom-Json
Conferir (@($final.recusadas) -contains $vC) "a $vC está nas recusadas: $(Mostrar (Ler $estado))"
if ($vitral) {
    Start-Sleep -Seconds 20
    Frente $vitral
    Print '12-voltou-para-a-b'
    Fechar $vitral "a $vB"
}
Anotar "pasta: $((Get-ChildItem $raiz -Force -Name) -join ', ')"
Anotar "atualizacoes.json: $(Mostrar (Ler $config))"
Anotar "pedidos: $(Mostrar (Ler $pedidos))"
Copy-Item $pedidos (Join-Path $Saida 'pedidos.log') -ErrorAction SilentlyContinue
Stop-Process -Id $servidor.Id -Force -ErrorAction SilentlyContinue

if ($falhas.Count -gt 0) {
    Anotar "$($falhas.Count) conferência(s) falharam: $($falhas -join '; ')"
    exit 1
}
Anotar 'fim: tudo conferido'
