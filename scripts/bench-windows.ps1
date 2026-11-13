<#
.SYNOPSIS
  Start time and memory at rest of a desktop client on Windows, for the side by side table of
  docs/dev/BENCHMARKS.md. The Windows counterpart of `scripts/bench.py other`.

.EXAMPLE
  .\scripts\bench-windows.ps1 -Name PlatypusDB -Exe target\release\platypus-db.exe -Kind platypus
  .\scripts\bench-windows.ps1 -Name DBeaver -Exe 'C:\Program Files\DBeaver\dbeaver.exe' -Kind dbeaver
  .\scripts\bench-windows.ps1 -Name DataGrip -Exe "C:\Program Files\JetBrains\DataGrip 2026.2.6\bin\datagrip64.exe" -Kind jetbrains

.DESCRIPTION
  Five runs per client. Each run, in a profile of its own (a temporary directory that holds
  the client's config, data and caches, never the user's real one):
    1. waits for the machine to be idle (no cargo, rustc, node, tauri process and total CPU
       under 15 % for 5 s);
    2. launches the client and captures the rectangle of its main window every 100 ms;
    3. start = launch to the first of the frames after which the window's pixels stay
       unchanged for 1 s (60 s cap);
    4. memory at rest = sum of the private bytes of the process tree, 5 s after the window
       settled, with no connection open;
    5. closes the client (the process tree is killed).
  The profile is shared by the five runs of a client, so run 1 is a first start (empty
  profile, first-run caches) and runs 2-5 are warm starts; both are printed.
  The window is captured from the screen (CopyFromScreen), so keep it in front and the desktop
  untouched while it runs. Median and range are printed as a markdown row.
#>
param(
  [Parameter(Mandatory)] [string] $Name,
  [Parameter(Mandatory)] [string] $Exe,
  [ValidateSet('platypus', 'dbeaver', 'jetbrains', 'plain')] [string] $Kind = 'plain',
  [int] $Runs = 5,
  [int] $SettleMs = 1000,
  [int] $CapS = 60,
  [int] $RestS = 5,
  [switch] $SkipIdleCheck
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

public static class WinCap {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr p);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
  static WinCap() { SetProcessDPIAware(); }

  // The biggest visible top-level window of any of `pids`, at least 640x400: the main window
  // (splash screens and small dialogs are smaller or are replaced by it).
  public static IntPtr MainWindow(HashSet<uint> pids, out RECT best) {
    IntPtr found = IntPtr.Zero; RECT bestRect = new RECT(); long bestArea = 0;
    EnumWindows((h, p) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      if (!pids.Contains(pid) || !IsWindowVisible(h) || IsIconic(h)) return true;
      RECT r; GetWindowRect(h, out r);
      int w = r.R - r.L, hh = r.B - r.T;
      if (w < 640 || hh < 400) return true;
      long a = (long)w * hh;
      if (a > bestArea) { bestArea = a; found = h; bestRect = r; }
      return true;
    }, IntPtr.Zero);
    best = bestRect; return found;
  }

  // Hash of the pixels on screen inside the rectangle.
  public static string Hash(RECT r) {
    int w = r.R - r.L, h = r.B - r.T;
    using (var bmp = new Bitmap(w, h, PixelFormat.Format32bppArgb)) {
      using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(r.L, r.T, 0, 0, new Size(w, h));
      var data = bmp.LockBits(new Rectangle(0, 0, w, h), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
      try {
        var bytes = new byte[Math.Abs(data.Stride) * h];
        Marshal.Copy(data.Scan0, bytes, 0, bytes.Length);
        return Convert.ToBase64String(MD5.Create().ComputeHash(bytes)) + ":" + w + "x" + h;
      } finally { bmp.UnlockBits(data); }
    }
  }
}
'@ -ReferencedAssemblies System.Drawing

$Exe = (Resolve-Path $Exe).Path

function Wait-Idle {
  if ($SkipIdleCheck) { return }
  while ($true) {
    $running = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { 'cargo', 'rustc', 'tauri', 'link', 'cl' -contains $_.ProcessName })
    $names = @($running | Select-Object -Expand ProcessName -Unique)
    # node only counts when it runs the E2E runner or a tauri build
    $nodes = @(Get-CimInstance Win32_Process -Filter "Name='node.exe'" | Where-Object { $_.CommandLine -match 'e2e[\\/]run\.mjs|tauri' })
    if ($nodes) { $names += 'node' }
    if ($names) {
      Write-Host "waiting: $($names -join ', ') running"
      Start-Sleep -Seconds 10; continue
    }
    # WMI, not Get-Counter: counter names are localized
    $samples = 1..5 | ForEach-Object {
      (Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'").PercentProcessorTime
      Start-Sleep -Seconds 1
    }
    $cpu = ($samples | Measure-Object -Average).Average
    if ($cpu -lt 15) { return }
    Write-Host ("waiting: CPU {0:N0} %" -f $cpu)
    Start-Sleep -Seconds 5
  }
}

function Get-Tree([int] $RootPid) {
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
  $ids = [System.Collections.Generic.HashSet[uint32]]::new()
  [void] $ids.Add([uint32] $RootPid)
  do {
    $before = $ids.Count
    foreach ($p in $all) { if ($ids.Contains([uint32] $p.ParentProcessId)) { [void] $ids.Add([uint32] $p.ProcessId) } }
  } while ($ids.Count -ne $before)
  , $ids
}

function Get-PrivateMB($ids) {
  $sum = 0
  foreach ($id in $ids) {
    $p = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($p) { $sum += $p.PrivateMemorySize64 }
  }
  [math]::Round($sum / 1MB, 1)
}

function Stop-Tree([int] $RootPid) {
  & taskkill /PID $RootPid /T /F *> $null
  Start-Sleep -Milliseconds 1500
}

# One profile per client under a temporary root; the user's own config is never touched.
$root = Join-Path ([IO.Path]::GetTempPath()) ("bench-" + $Name + "-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force "$root\home\AppData\Roaming", "$root\home\AppData\Local", "$root\ws" | Out-Null
$env:USERPROFILE = "$root\home"
$env:HOME = "$root\home"
$env:APPDATA = "$root\home\AppData\Roaming"
$env:LOCALAPPDATA = "$root\home\AppData\Local"
$env:TEMP = "$root\home\AppData\Local\Temp"; $env:TMP = $env:TEMP
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
$launchArgs = @()
switch ($Kind) {
  'platypus' {
    $env:PLATYPUS_DATA_DIR = "$root\data"
    New-Item -ItemType Directory -Force $env:PLATYPUS_DATA_DIR | Out-Null
    $env:WEBVIEW2_USER_DATA_FOLDER = "$root\webview2"
  }
  'dbeaver' {
    # A copy of the portable install's configuration directory, so it is not written to.
    Copy-Item (Join-Path (Split-Path $Exe) 'configuration') "$root\cfg" -Recurse
    $launchArgs = @('-nosplash', '-data', "$root\ws", '-configuration', "$root\cfg",
      '-vmargs', "-Duser.home=$root\home")
  }
  'jetbrains' {
    $env:IDEA_PROPERTIES = "$root\idea.properties"
    @("idea.config.path=$($root.Replace('\','/'))/cfg", "idea.system.path=$($root.Replace('\','/'))/sys",
      "idea.plugins.path=$($root.Replace('\','/'))/cfg/plugins", "idea.log.path=$($root.Replace('\','/'))/log") |
      Set-Content $env:IDEA_PROPERTIES
  }
}

$results = @()
try {
  for ($run = 1; $run -le $Runs; $run++) {
    Wait-Idle
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $launchSplat = @{ FilePath = $Exe; PassThru = $true }
    if ($launchArgs.Count) { $launchSplat.ArgumentList = $launchArgs }
    $proc = Start-Process @launchSplat
    $last = $null; $stableSince = $null; $firstStable = $null; $settled = $false
    while ($sw.Elapsed.TotalSeconds -lt $CapS) {
      $ids = Get-Tree $proc.Id
      $rect = New-Object WinCap+RECT
      $hwnd = [WinCap]::MainWindow($ids, [ref] $rect)
      if ($hwnd -ne [IntPtr]::Zero) {
        $now = $sw.Elapsed.TotalMilliseconds
        $h = [WinCap]::Hash($rect)
        if ($h -ne $last) { $last = $h; $firstStable = $now; $stableSince = $now }
        elseif (($now - $stableSince) -ge $SettleMs) { $settled = $true; break }
      }
      Start-Sleep -Milliseconds 100
    }
    if (-not $settled) { Write-Host "run ${run}: window did not settle in $CapS s"; Stop-Tree $proc.Id; continue }
    $startMs = [math]::Round($firstStable)
    Start-Sleep -Seconds $RestS
    $ids = Get-Tree $proc.Id
    $mb = Get-PrivateMB $ids
    Write-Host "run ${run}: start $startMs ms, private $mb MB, $($ids.Count) processes"
    $results += [pscustomobject]@{ Run = $run; StartMs = $startMs; PrivateMB = $mb; Procs = $ids.Count }
    Stop-Tree $proc.Id
  }
} finally {
  Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
}

function Stat($values) {
  $s = @($values | Sort-Object)
  if (-not $s) { return 'n/a' }
  $mid = [math]::Floor($s.Count / 2)
  $median = if ($s.Count % 2) { $s[$mid] } else { ($s[$mid - 1] + $s[$mid]) / 2 }
  "median $median (range $($s[0])-$($s[-1]))"
}
Write-Host ""
Write-Host "$Name, $($results.Count) runs:"
Write-Host "  start ms, all runs:   $(Stat $results.StartMs)"
if ($results.Count -gt 1) { Write-Host "  start ms, warm (2+):  $(Stat ($results | Where-Object Run -gt 1).StartMs)" }
Write-Host "  private MB at rest:   $(Stat $results.PrivateMB)"
$results | Format-Table -AutoSize
