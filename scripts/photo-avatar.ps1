# Photographie l'écran RÉEL dans le rectangle de la fenêtre de l'avatar :
# ce que voit l'utilisateur, découpe de la fenêtre comprise (piège 85).
# `/snapshot` lit l'image AVANT cette découpe : il montre ce qui est caché.
#
#   .\scripts\photo-avatar.ps1 -Out C:\chemin\photo.png
param([string]$Out)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[Win]::SetProcessDPIAware() | Out-Null
$godot = Get-Process Godot* | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $godot) { throw "fenêtre Godot introuvable" }
$r = New-Object Win+RECT
[Win]::GetWindowRect($godot.MainWindowHandle, [ref]$r) | Out-Null
$w = $r.R - $r.L; $h = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size $w, $h))
$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
"fenêtre $w x $h en ($($r.L), $($r.T))"
