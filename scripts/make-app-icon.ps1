# Erzeugt app-icon.png (1024x1024) im Stil der App: Violett-Verlauf, Glasrand, Mint-Akzent.
# Danach: npx tauri icon app-icon.png
param([string]$Out = "$PSScriptRoot/../app-icon.png")

Add-Type -AssemblyName System.Drawing
$size = 1024
$bmp = New-Object System.Drawing.Bitmap $size, $size
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.TextRenderingHint = 'AntiAliasGridFit'
$g.Clear([System.Drawing.Color]::Transparent)

function RoundRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

$m = 48
$shape = RoundRect $m $m ($size - 2 * $m) ($size - 2 * $m) 220
$grad = New-Object System.Drawing.Drawing2D.LinearGradientBrush (New-Object System.Drawing.Point 0, 0), (New-Object System.Drawing.Point $size, $size), ([System.Drawing.Color]::FromArgb(255, 139, 92, 246)), ([System.Drawing.Color]::FromArgb(255, 46, 16, 101))
$g.FillPath($grad, $shape)
$pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(110, 255, 255, 255)), 10
$g.DrawPath($pen, $shape)

# Glas-Karte
$card = RoundRect 230 300 564 470 70
$g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(60, 255, 255, 255))), $card)
$g.DrawPath((New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(150, 255, 255, 255)), 6), $card)

# Balken (lila) mit Mint-Akzent
$barBrush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(235, 255, 255, 255))
$mint = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 94, 234, 212))
$g.FillPath($barBrush, (RoundRect 300 560 90 150 24))
$g.FillPath($barBrush, (RoundRect 420 480 90 230 24))
$g.FillPath($mint, (RoundRect 540 390 90 320 24))
$g.FillPath($barBrush, (RoundRect 660 520 70 190 24))

# Glühpunkt
$g.FillEllipse($mint, 690, 190, 110, 110)

$g.Dispose()
$bmp.Save((Resolve-Path -LiteralPath (Split-Path $Out)).Path + "/" + (Split-Path $Out -Leaf), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "geschrieben: $Out"
