# Génère les icônes de l'application (PNG 32/128 + ICO Windows).
# Sans dépendance : System.Drawing de Windows suffit et le résultat est
# versionné, donc reproductible sur n'importe quelle machine.
param(
    [string]$OutputDir = (Join-Path $PSScriptRoot '..\desktop\src-tauri\icons')
)

Add-Type -AssemblyName System.Drawing

function New-FoxIcon([int]$Size) {
    $bmp = New-Object System.Drawing.Bitmap($Size, $Size)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)

    $u = $Size / 128.0
    $fur = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 214, 122, 46))
    $furDark = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 168, 78, 20))
    $cream = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 245, 233, 214))
    $dark = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 30, 22, 18))

    # Oreilles
    $g.FillPolygon($fur, @(
        (New-Object System.Drawing.Point([int](34 * $u), [int](46 * $u))),
        (New-Object System.Drawing.Point([int](20 * $u), [int](10 * $u))),
        (New-Object System.Drawing.Point([int](56 * $u), [int](30 * $u)))))
    $g.FillPolygon($fur, @(
        (New-Object System.Drawing.Point([int](94 * $u), [int](46 * $u))),
        (New-Object System.Drawing.Point([int](108 * $u), [int](10 * $u))),
        (New-Object System.Drawing.Point([int](72 * $u), [int](30 * $u)))))

    # Crâne
    $g.FillEllipse($fur, [int](24 * $u), [int](28 * $u), [int](80 * $u), [int](84 * $u))
    # Muzzle
    $g.FillEllipse($cream, [int](40 * $u), [int](70 * $u), [int](48 * $u), [int](38 * $u))
    # Nez
    $g.FillEllipse($dark, [int](57 * $u), [int](74 * $u), [int](14 * $u), [int](10 * $u))
    # Yeux
    $g.FillEllipse($dark, [int](42 * $u), [int](52 * $u), [int](12 * $u), [int](15 * $u))
    $g.FillEllipse($dark, [int](74 * $u), [int](52 * $u), [int](12 * $u), [int](15 * $u))
    # Écharpe
    $g.FillRectangle($furDark, [int](30 * $u), [int](104 * $u), [int](68 * $u), [int](10 * $u))

    $g.Dispose()
    return $bmp
}

function Save-Png($Bitmap, [string]$Path) {
    $Bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
}

function Save-Ico($Bitmap, [string]$Path) {
    # Un .ico moderne accepte une image PNG encodée telle quelle.
    $ms = New-Object System.IO.MemoryStream
    $Bitmap.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $png = $ms.ToArray()
    $ms.Dispose()

    $out = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($out)
    $size = [int]$Bitmap.Width
    $bw.Write([uint16]0)              # reserved
    $bw.Write([uint16]1)              # type : icône
    $bw.Write([uint16]1)              # une image
    $bw.Write([byte]($(if ($size -ge 256) { 0 } else { $size })))
    $bw.Write([byte]($(if ($size -ge 256) { 0 } else { $size })))
    $bw.Write([byte]0)                # palette
    $bw.Write([byte]0)                # réservé
    $bw.Write([uint16]1)              # plans
    $bw.Write([uint16]32)             # bits par pixel
    $bw.Write([uint32]$png.Length)
    $bw.Write([uint32]22)             # décalage de l'image
    $bw.Write($png)
    $bw.Flush()

    [System.IO.File]::WriteAllBytes($Path, $out.ToArray())
    $out.Dispose()
    $bw.Dispose()
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

$bmp32 = New-FoxIcon 32
Save-Png $bmp32 (Join-Path $OutputDir '32x32.png')
Save-Ico $bmp32 (Join-Path $OutputDir 'icon.ico')
$bmp32.Dispose()

foreach ($size in 128, 256, 512) {
    $bmp = New-FoxIcon $size
    Save-Png $bmp (Join-Path $OutputDir ("{0}x{0}.png" -f $size))
    $bmp.Dispose()
}

Write-Host "Icônes écrites dans $OutputDir"
Get-ChildItem $OutputDir | Select-Object Name, Length