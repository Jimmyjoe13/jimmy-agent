# Génère les icônes de l'application (PNG 32/128/256/512, ICO Windows
# multi-tailles, favicon) à partir du logo officiel : le monogramme « AJ »
# détouré (`desktop/src/assets/logo-mark.png`, tiré de `logo-agent-jimmy.jpg`)
# posé sur une tuile marine arrondie, lisible jusqu'en 16 px.
# Sans dépendance : System.Drawing de Windows suffit et le résultat est
# versionné, donc reproductible sur n'importe quelle machine.
param(
    [string]$OutputDir = (Join-Path $PSScriptRoot '..\desktop\src-tauri\icons'),
    [string]$Mark = (Join-Path $PSScriptRoot '..\desktop\src\assets\logo-mark.png'),
    [string]$Favicon = (Join-Path $PSScriptRoot '..\desktop\public\icons\32x32.png')
)

Add-Type -AssemblyName System.Drawing

if (-not (Test-Path $Mark)) { throw "Monogramme introuvable : $Mark" }
$markImage = [System.Drawing.Image]::FromFile((Resolve-Path $Mark))

# Fond marine du logo officiel.
$tileColor = [System.Drawing.Color]::FromArgb(255, 23, 30, 44)

function New-LogoIcon([int]$Size) {
    # Dessin en 4x puis réduction : les bords de la tuile restent nets en 16-32 px.
    $big = $Size * 4
    $canvas = New-Object System.Drawing.Bitmap($big, $big)
    $g = [System.Drawing.Graphics]::FromImage($canvas)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.Clear([System.Drawing.Color]::Transparent)

    # Tuile arrondie (rayon 22 % du côté).
    $d = [int]($big * 0.44)
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc(0, 0, $d, $d, 180, 90)
    $path.AddArc($big - $d - 1, 0, $d, $d, 270, 90)
    $path.AddArc($big - $d - 1, $big - $d - 1, $d, $d, 0, 90)
    $path.AddArc(0, $big - $d - 1, $d, $d, 90, 90)
    $path.CloseFigure()
    $brush = New-Object System.Drawing.SolidBrush($tileColor)
    $g.FillPath($brush, $path)

    # Monogramme centré, 80 % du côté.
    $inner = [int]($big * 0.80)
    $offset = [int](($big - $inner) / 2)
    $g.DrawImage($markImage, $offset, $offset, $inner, $inner)

    $brush.Dispose(); $path.Dispose(); $g.Dispose()

    # Réduction à la taille finale.
    $bmp = New-Object System.Drawing.Bitmap($Size, $Size)
    $g2 = [System.Drawing.Graphics]::FromImage($bmp)
    $g2.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g2.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g2.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g2.Clear([System.Drawing.Color]::Transparent)
    $g2.DrawImage($canvas, 0, 0, $Size, $Size)
    $g2.Dispose(); $canvas.Dispose()
    return $bmp
}

function Get-PngBytes($Bitmap) {
    $ms = New-Object System.IO.MemoryStream
    $Bitmap.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bytes = $ms.ToArray()
    $ms.Dispose()
    return , $bytes
}

function Save-Png($Bitmap, [string]$Path) {
    $Bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
}

function Save-Ico([int[]]$Sizes, [string]$Path) {
    # .ico moderne multi-tailles : chaque image est un PNG encodé tel quel.
    $images = foreach ($s in $Sizes) {
        $bmp = New-LogoIcon $s
        , (Get-PngBytes $bmp)
        $bmp.Dispose()
    }

    $out = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($out)
    $bw.Write([uint16]0)              # réservé
    $bw.Write([uint16]1)              # type : icône
    $bw.Write([uint16]$Sizes.Count)   # nombre d'images

    # Répertoire : 16 octets par image, données à la suite.
    $dataOffset = 6 + 16 * $Sizes.Count
    for ($i = 0; $i -lt $Sizes.Count; $i++) {
        $s = $Sizes[$i]
        $png = $images[$i]
        $dim = $(if ($s -ge 256) { 0 } else { $s })  # 0 = 256 px
        $bw.Write([byte]$dim)
        $bw.Write([byte]$dim)
        $bw.Write([byte]0)            # palette
        $bw.Write([byte]0)            # réservé
        $bw.Write([uint16]1)          # plans
        $bw.Write([uint16]32)         # bits par pixel
        $bw.Write([uint32]$png.Length)
        $bw.Write([uint32]$dataOffset)
        $dataOffset += $png.Length
    }
    foreach ($png in $images) { $bw.Write($png) }
    $bw.Flush()

    [System.IO.File]::WriteAllBytes($Path, $out.ToArray())
    $bw.Dispose()
    $out.Dispose()
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

foreach ($size in 32, 128, 256, 512) {
    $bmp = New-LogoIcon $size
    Save-Png $bmp (Join-Path $OutputDir ("{0}x{0}.png" -f $size))
    # Le favicon de l'interface est l'icône 32 px.
    if ($size -eq 32) { Save-Png $bmp $Favicon }
    $bmp.Dispose()
}
Save-Ico @(16, 24, 32, 48, 64, 128, 256) (Join-Path $OutputDir 'icon.ico')

$markImage.Dispose()

Write-Host "Icônes écrites dans $OutputDir (+ favicon $Favicon)"
Get-ChildItem $OutputDir | Select-Object Name, Length
