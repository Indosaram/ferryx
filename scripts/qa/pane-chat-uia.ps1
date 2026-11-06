# UI Automation driver for the Ferryx pane chat QA window.
# Runs in the interactive Windows desktop session; writes JSON receipts next to the evidence root.
param(
    [string]$Mode = 'dump',
    [string]$WindowPid = '',
    [string]$Match = '',
    [int]$Limit = 20000,
    [int]$X = 0,
    [int]$Y = 0,
    [string]$Keys = ''
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$evidence = 'C:\Users\sook\ferryx-wt\.evidence\pane-chat-mode-20261104'
$outDir = Join-Path $evidence 'windows-profile-cdp\uia'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$AE = [System.Windows.Automation.AutomationElement]

function Get-AppWindow([string]$pidText) {
    $condition = [System.Windows.Automation.PropertyCondition]::new($AE::ProcessIdProperty, [int]$pidText)
    $wins = $AE::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
    foreach ($w in $wins) {
        if ($w.Current.ControlType -eq [System.Windows.Automation.ControlType]::Window) { return $w }
    }
    if ($wins.Count -gt 0) { return $wins[0] }
    throw "no automation window for pid $pidText"
}

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class QaWindow {
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int cmd);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();

}
public class QaMouse {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(150);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(80);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Hover(int x, int y) { SetCursorPos(x, y); }
    public static void RightClick(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(150);
        mouse_event(0x0008, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(80);
        mouse_event(0x0010, 0, 0, 0, UIntPtr.Zero);
    }
}
"@

function Focus-AppWindow([System.Windows.Automation.AutomationElement]$window) {
    $hwnd = [IntPtr]$window.Current.NativeWindowHandle
    if ($hwnd -eq [IntPtr]::Zero) { throw 'window has no native handle' }
    if ([QaWindow]::IsIconic($hwnd)) { [void][QaWindow]::ShowWindow($hwnd, 9) }
    # Attach to the foreground thread so SetForegroundWindow is allowed for a background process.
    $fg = [QaWindow]::GetForegroundWindow()
    $fgPid = 0
    $fgThread = [QaWindow]::GetWindowThreadProcessId($fg, [ref]$fgPid)
    $me = [QaWindow]::GetCurrentThreadId()
    if ($fgThread -ne 0 -and $fgThread -ne $me) { [void][QaWindow]::AttachThreadInput($fgThread, $me, $true) }
    [void][QaWindow]::BringWindowToTop($hwnd)
    $ok = [QaWindow]::SetForegroundWindow($hwnd)
    if ($fgThread -ne 0 -and $fgThread -ne $me) { [void][QaWindow]::AttachThreadInput($fgThread, $me, $false) }
    Start-Sleep -Milliseconds 350
    return [pscustomobject]@{ ok = $ok; hwnd = $hwnd.ToInt64(); foreground = ([QaWindow]::GetForegroundWindow() -eq $hwnd) }
}

function Walk([System.Windows.Automation.AutomationElement]$node, [int]$depth, [System.Collections.Generic.List[object]]$acc) {
    if ($acc.Count -ge $Limit) { return }
    $cur = $node.Current
    $rect = $cur.BoundingRectangle
    # Offscreen/virtualized elements report infinite rectangles; clamp before int conversion.
    $x = 0; $y = 0; $w = 0; $h = 0
    if (-not [double]::IsInfinity($rect.X) -and -not [double]::IsNaN($rect.X)) { $x = [int][math]::Round($rect.X) }
    if (-not [double]::IsInfinity($rect.Y) -and -not [double]::IsNaN($rect.Y)) { $y = [int][math]::Round($rect.Y) }
    if (-not [double]::IsInfinity($rect.Width) -and -not [double]::IsNaN($rect.Width)) { $w = [int][math]::Round($rect.Width) }
    if (-not [double]::IsInfinity($rect.Height) -and -not [double]::IsNaN($rect.Height)) { $h = [int][math]::Round($rect.Height) }
    $acc.Add([pscustomobject]@{
        depth     = $depth
        name      = $cur.Name
        type      = $cur.ControlType.ProgrammaticName
        autoId    = $cur.AutomationId
        className = $cur.ClassName
        offscreen = $cur.IsOffscreen
        enabled   = $cur.IsEnabled
        x         = $x
        y         = $y
        w         = $w
        h         = $h
    })
    if ($depth -ge 40) { return }
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $child = $walker.GetFirstChild($node)
    while ($null -ne $child) {
        Walk $child ($depth + 1) $acc
        $child = $walker.GetNextSibling($child)
    }
}

$window = Get-AppWindow $WindowPid
$stamp = Get-Date -Format 'HHmmss'
if ($Mode -eq 'windows') {
    $procs = @{}
    foreach ($p in Get-Process) { $procs[$p.Id] = $p.ProcessName }
    $rows = @()
    foreach ($w in [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
            [System.Windows.Automation.TreeScope]::Children,
            [System.Windows.Automation.Condition]::TrueCondition)) {
        $wpid = $w.Current.ProcessId
        $r = $w.Current.BoundingRectangle
        $rx = if ([double]::IsInfinity($r.X)) { -1 } else { [int]$r.X }
        $ry = if ([double]::IsInfinity($r.Y)) { -1 } else { [int]$r.Y }
        $rw = if ([double]::IsInfinity($r.Width)) { -1 } else { [int]$r.Width }
        $rh = if ([double]::IsInfinity($r.Height)) { -1 } else { [int]$r.Height }
        $rows += [pscustomobject]@{
            pid = $wpid
            proc = $procs[$wpid]
            cls = $w.Current.ClassName
            name = $w.Current.Name
            type = $w.Current.ControlType.ProgrammaticName
            hwnd = $w.Current.NativeWindowHandle
            rect = "$rx,$ry $rw x $rh"
        }
    }
    $rows | ConvertTo-Json -Depth 4
    exit 0
}

if ($Mode -eq 'dumpwin') {
    $acc = New-Object 'System.Collections.Generic.List[object]'
    Walk $window 0 $acc
    $acc | ConvertTo-Json -Depth 4
    exit 0
}

if ($Mode -eq 'values') {
    $acc = New-Object 'System.Collections.Generic.List[object]'
    Walk $window 0 $acc
    $rows = @()
    foreach ($n in $acc) {
        $isEdit = $n.type -match 'Edit|Document|Text'
        if (-not $isEdit) { continue }
        $val = $null
        $pat = $null
        if ($n.element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pat)) {
            $val = $pat.Current.Value
        }
        $rows += [pscustomobject]@{ type = $n.type; name = $n.name; value = $val }
    }
    $rows | ConvertTo-Json -Depth 3
    exit 0
}

if ($Mode -eq 'focus') {
    [pscustomobject](Focus-AppWindow $window) | ConvertTo-Json
    exit 0
}

if ($Mode -eq 'dump') {
    $acc = [System.Collections.Generic.List[object]]::new()
    Walk $window 0 $acc
    $path = Join-Path $outDir "tree-$stamp.json"
    $acc | ConvertTo-Json -Depth 4 | Set-Content -Path $path -Encoding UTF8
    $named = $acc | Where-Object { $_.name -and $_.name.Trim() -ne '' } | Where-Object { -not $_.offscreen }
    [pscustomobject]@{
        ok = $true; mode = 'dump'; tree = $path; nodes = $acc.Count; named = @($named).Count
        matches = @($named | Where-Object { $_.name -match $Match } | Select-Object -First 60)
    } | ConvertTo-Json -Depth 5
    exit 0
}

if ($Mode -eq 'invoke') {
    $acc = [System.Collections.Generic.List[object]]::new()
    Walk $window 0 $acc
    $target = $acc | Where-Object { $_.name -eq $Match -and -not $_.offscreen } | Select-Object -First 1
    if (-not $target) { throw "no element named '$Match'" }
    $condition = [System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new($AE::NameProperty, $Match),
        [System.Windows.Automation.PropertyCondition]::new($AE::ProcessIdProperty, [int]$WindowPid)
    )
    $found = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
    $pattern = $null
    if ($found -and $found.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
        [pscustomobject]@{ ok = $true; mode = 'invoke'; via = 'invoke-pattern'; name = $Match } | ConvertTo-Json
        exit 0
    }
    [pscustomobject]@{ ok = $false; mode = 'invoke'; via = 'none'; name = $Match; note = 'no invoke pattern; use click mode' } | ConvertTo-Json
    exit 0
}

if ($Mode -eq 'click') {
    $acc = [System.Collections.Generic.List[object]]::new()
    Walk $window 0 $acc
    $target = $acc | Where-Object { $_.name -eq $Match -and -not $_.offscreen } | Select-Object -First 1
    if (-not $target) { throw "no element named '$Match'" }
    if ($target.w -le 0 -or $target.h -le 0) { throw "element '$Match' has no clickable bounds" }
    $focus = Focus-AppWindow $window
    $cx = $target.x + [int]($target.w / 2)
    $cy = $target.y + [int]($target.h / 2)
    [QaMouse]::Click($cx, $cy)
    [pscustomobject]@{ ok = $true; mode = 'click'; name = $Match; x = $cx; y = $cy; focused = $focus.ok; foreground = $focus.foreground } | ConvertTo-Json
    exit 0
}

if ($Mode -eq 'clickxy') {
    # Real input injection so OS-native popup menus (separate HWNDs) receive the click.
    [QaMouse]::Hover($X, $Y)
    Start-Sleep -Milliseconds 400
    [QaMouse]::Click($X, $Y)
    [pscustomobject]@{ ok = $true; mode = 'clickxy'; x = $X; y = $Y } | ConvertTo-Json
    exit 0
}

if ($Mode -eq 'hoverxy') {
    [QaMouse]::Hover($X, $Y)
    [pscustomobject]@{ ok = $true; mode = 'hoverxy'; x = $X; y = $Y } | ConvertTo-Json
    exit 0
}

if ($Mode -eq 'keys') {
    Add-Type -AssemblyName System.Windows.Forms
    [System.Windows.Forms.SendKeys]::SendWait($Keys)
    [pscustomobject]@{ ok = $true; mode = 'keys'; keys = $Keys } | ConvertTo-Json
    exit 0
}

if ($Mode -eq 'rightkeys') {
    Add-Type -AssemblyName System.Windows.Forms
    $focus = Focus-AppWindow $window
    [QaMouse]::Hover($X, $Y)
    Start-Sleep -Milliseconds 300
    [QaMouse]::RightClick($X, $Y)
    Start-Sleep -Milliseconds 900
    foreach ($chunk in $Keys.Split('|')) {
        if ($chunk) { [System.Windows.Forms.SendKeys]::SendWait($chunk) }
        Start-Sleep -Milliseconds 450
    }
    [pscustomobject]@{ ok = $true; mode = 'rightkeys'; keys = $Keys; foreground = $focus.foreground } | ConvertTo-Json
    exit 0
}

if ($Mode -eq 'typeat') {
    # Focus the app window, click a point, then type into whatever got focus.
    Add-Type -AssemblyName System.Windows.Forms
    $focus = Focus-AppWindow $window
    if ($X -ne 0 -or $Y -ne 0) {
        [QaMouse]::Hover($X, $Y)
        Start-Sleep -Milliseconds 250
        [QaMouse]::Click($X, $Y)
        Start-Sleep -Milliseconds 600
    }
    foreach ($chunk in $Keys.Split('|')) {
        if ($chunk) { [System.Windows.Forms.SendKeys]::SendWait($chunk) }
        Start-Sleep -Milliseconds 250
    }
    Start-Sleep -Milliseconds 500
    [pscustomobject]@{ ok = $true; mode = 'typeat'; x = $X; y = $Y; foreground = $focus.foreground } | ConvertTo-Json
    exit 0
}

throw "unknown mode $Mode"
