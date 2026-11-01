# Prints do Vitral num Windows de verdade (o runner windows-2025 do GitHub, sem GPU: o DirectX 11 do
# GPUI cai no WARP, o renderizador por software). Roda no .github/workflows/windows-prints.yml.
#
# Uso: ferramentas/prints-windows.ps1 <vitral.exe> <pasta_dos_prints>
# Abre o Vitral com a pasta de demonstração (ferramentas/demo_vitral.py) e tira prints da tela
# inteira. No caminho, confere as áreas que o Windows vê na janela (WM_NCHITTEST na faixa de cima: 2
# = arrastar, 8 = minimizar, 9 = maximizar, 20 = fechar, 1 = conteúdo), maximiza pelo □, restaura
# com o clique duplo na faixa, abre uma conversa e fecha pelo ×. O que acontece vai para
# <pasta_dos_prints>/roteiro.log; o stderr do Vitral, para vitral-stderr.log.
param(
    [Parameter(Mandatory)] [string] $Vitral,
    [Parameter(Mandatory)] [string] $Saida
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Saida | Out-Null
$Saida = (Resolve-Path $Saida).Path
$Vitral = (Resolve-Path $Vitral).Path
$log = Join-Path $Saida 'roteiro.log'
function Anotar([string] $texto) {
    $linha = "[{0:HH:mm:ss}] {1}" -f (Get-Date), $texto
    Write-Host $linha
    Add-Content -Path $log -Value $linha -Encoding utf8
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
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
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
    public static void Clicar(int x, int y, int vezes) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(200);
        for (int i = 0; i < vezes; i++) {
            mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
            mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
            System.Threading.Thread.Sleep(60);
        }
    }
    // O GPUI responde o WM_NCHITTEST pelo último lugar do ponteiro que ele viu: mexe o ponteiro
    // até lá antes de perguntar.
    public static int Area(IntPtr janela, int x, int y) {
        SetCursorPos(x, y - 1);
        System.Threading.Thread.Sleep(120);
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(120);
        return SendMessage(janela, 0x0084, IntPtr.Zero, (IntPtr)((y << 16) | (x & 0xFFFF))).ToInt32();
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

Anotar ("resolução 1920x1080: ChangeDisplaySettings = {0}" -f [Tela]::Resolucao(1920, 1080))
$tela = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
Anotar ("tela: {0}x{1}" -f $tela.Width, $tela.Height)
# Um papel de parede com cor, para o acrílico ter o que desfocar.
$papel = Get-ChildItem 'C:\Windows\Web\Wallpaper' -Recurse -Include *.jpg -ErrorAction SilentlyContinue | Select-Object -First 1
if ($papel) {
    Anotar ("papel de parede {0}: {1}" -f $papel.FullName, [Tela]::SystemParametersInfo(0x0014, 0, $papel.FullName, 3))
}
Anotar ("--versao: {0}" -f ((& $Vitral --versao) -join ' '))

$demo = Join-Path $env:RUNNER_TEMP 'vitral-demo'
python -I ferramentas/demo_vitral.py $demo 2>&1 | ForEach-Object { Anotar "demo: $_" }
$argumentos = @()
if (Test-Path (Join-Path $demo '.vitral')) { $argumentos = @($demo) } else { Anotar 'sem a pasta de demonstração: abre como primeiro uso' }

Print '00-antes'
$p = Start-Process -FilePath $Vitral -ArgumentList $argumentos -PassThru -NoNewWindow `
    -RedirectStandardError (Join-Path $Saida 'vitral-stderr.log') -RedirectStandardOutput (Join-Path $Saida 'vitral-stdout.log')
$janela = [IntPtr]::Zero
foreach ($i in 1..90) {
    Start-Sleep -Seconds 1
    $p.Refresh()
    if ($p.HasExited) { break }
    if ($p.MainWindowHandle -ne [IntPtr]::Zero) { $janela = $p.MainWindowHandle; break }
}
if ($janela -eq [IntPtr]::Zero) {
    Anotar ("a janela não apareceu (saiu: {0}, código {1})" -f $p.HasExited, $(if ($p.HasExited) { $p.ExitCode } else { '-' }))
    Print '01-sem-janela'
    exit 0
}
Anotar ("janela {0} depois de {1} s; título '{2}'" -f $janela, $i, $p.MainWindowTitle)
# A migração, a importação e o primeiro quadro no WARP.
Start-Sleep -Seconds 20
[void][Tela]::SetForegroundWindow($janela)
Start-Sleep -Seconds 2
Print '01-abertura'

$r = New-Object Tela+RECT
[void][Tela]::GetWindowRect($janela, [ref] $r)
Anotar ("retângulo: ({0},{1}) a ({2},{3}), maximizada: {4}" -f $r.Left, $r.Top, $r.Right, $r.Bottom, [Tela]::IsZoomed($janela))

# A faixa de cima, da direita para a esquerda: onde ficam o ×, o □, o – e a área de arrastar.
$achadas = @{}
foreach ($y in @(($r.Top + 20), ($r.Top + 28), ($r.Top + 36))) {
    $linha = @()
    for ($x = $r.Right - 8; $x -gt $r.Right - 200; $x -= 6) {
        $area = [Tela]::Area($janela, $x, $y)
        $linha += "$($r.Right - $x):$area"
        if (-not $achadas.ContainsKey($area)) { $achadas[$area] = @($x, $y) }
    }
    Anotar ("áreas em y=+{0} (distância da direita:código): {1}" -f ($y - $r.Top), ($linha -join ' '))
}
$meio = [int](($r.Left + $r.Right) / 2)
Anotar ("no meio da faixa ({0},+28): {1}; no conteúdo (+400): {2}" -f $meio, [Tela]::Area($janela, $meio, $r.Top + 28), [Tela]::Area($janela, $meio, $r.Top + 400))

if ($achadas.ContainsKey(20)) {
    [void][Tela]::SetCursorPos($achadas[20][0], $achadas[20][1])
    Start-Sleep -Milliseconds 600
    Print '02-ponteiro-no-fechar'
}
if ($achadas.ContainsKey(9)) {
    [Tela]::Clicar($achadas[9][0], $achadas[9][1], 1)
    Start-Sleep -Seconds 3
    Anotar ("clique no □: maximizada = {0}" -f [Tela]::IsZoomed($janela))
    Print '03-maximizada'
    [void][Tela]::GetWindowRect($janela, [ref] $r)
    $meio = [int](($r.Left + $r.Right) / 2)
    Anotar ("área no meio da faixa maximizada: {0}" -f [Tela]::Area($janela, $meio, $r.Top + 28))
    [Tela]::Clicar($meio, $r.Top + 28, 2)
    Start-Sleep -Seconds 3
    Anotar ("clique duplo na faixa: maximizada = {0}" -f [Tela]::IsZoomed($janela))
    Print '04-restaurada'
} else {
    Anotar 'o □ não apareceu como área do Windows (9)'
}

# Uma conversa com markdown e código: o segundo cartão da lista (o primeiro é o Mestre ou "Esperando você").
[void][Tela]::GetWindowRect($janela, [ref] $r)
[Tela]::Clicar($r.Left + 200, $r.Top + 330, 1)
Start-Sleep -Seconds 4
Print '05-conversa'
[Tela]::Clicar($r.Left + 200, $r.Top + 140, 1)
Start-Sleep -Seconds 4
Print '06-outra-conversa'

# O × (a janela voltou para o mesmo lugar depois de restaurada).
if ($achadas.ContainsKey(20)) {
    [Tela]::Clicar($achadas[20][0], $achadas[20][1], 1)
    Start-Sleep -Seconds 4
    $p.Refresh()
    Anotar ("clique no ×: o Vitral saiu = {0}" -f $p.HasExited)
    Print '07-depois-do-fechar'
}
if (-not $p.HasExited) {
    Stop-Process -Id $p.Id -Force
    Anotar 'o Vitral ainda estava aberto: parado à força'
}

# A vitrine dos componentes: todos os pesos da Inter e o código em JetBrains Mono.
$v = Start-Process -FilePath $Vitral -ArgumentList @('--vitrine') -PassThru -NoNewWindow `
    -RedirectStandardError (Join-Path $Saida 'vitrine-stderr.log')
Start-Sleep -Seconds 25
Print '08-vitrine'
if (-not $v.HasExited) { Stop-Process -Id $v.Id -Force }
Anotar 'fim'
