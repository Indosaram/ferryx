Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class FGProbe {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
}
'@
$h = [FGProbe]::GetForegroundWindow()
$p = 0
[void][FGProbe]::GetWindowThreadProcessId($h, [ref]$p)
$sb = New-Object System.Text.StringBuilder 256
[void][FGProbe]::GetClassName($h, $sb, 256)
$name = (Get-Process -Id $p -ErrorAction SilentlyContinue).ProcessName
@{ processName = $name; className = $sb.ToString() } | ConvertTo-Json -Compress
