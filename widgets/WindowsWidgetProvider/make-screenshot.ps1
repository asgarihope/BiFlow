Add-Type -AssemblyName System.Drawing

$out = 'C:\www\BiFlow\widgets\WindowsWidgetProvider\Assets\WidgetScreenshot.png'
$w = 300; $h = 304

$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit
$g.Clear([System.Drawing.Color]::Transparent)

# Transparent, rounded corners - the picker requires it.
$radius = 22
$path = New-Object System.Drawing.Drawing2D.GraphicsPath
$path.AddArc(0, 0, $radius * 2, $radius * 2, 180, 90)
$path.AddArc($w - $radius * 2, 0, $radius * 2, $radius * 2, 270, 90)
$path.AddArc($w - $radius * 2, $h - $radius * 2, $radius * 2, $radius * 2, 0, 90)
$path.AddArc(0, $h - $radius * 2, $radius * 2, $radius * 2, 90, 90)
$path.CloseFigure()
$g.SetClip($path)

$bg = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 243, 243, 246))
$g.FillRectangle($bg, 0, 0, $w, $h)

$fg = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 32, 33, 36))
$subtle = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 110, 112, 118))
$cyan = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 0, 120, 212))
$amber = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 190, 110, 0))

$big = New-Object System.Drawing.Font 'Segoe UI', 30, ([System.Drawing.FontStyle]::Bold)
$mid = New-Object System.Drawing.Font 'Segoe UI', 20, ([System.Drawing.FontStyle]::Bold)
$small = New-Object System.Drawing.Font 'Segoe UI', 9
$caption = New-Object System.Drawing.Font 'Segoe UI', 8

$g.DrawString('Split active', $big, $fg, 20, 20)
$g.DrawString('Routing via 3 client(s), default Windscribe', $small, $subtle, 20, 62)

$g.FillRectangle($subtle, 20, 84, 260, 1)

# Two columns: download | upload, matching the medium Adaptive Card.
$g.DrawString('Download', $caption, $subtle, 20, 100)
$g.DrawString('Upload', $caption, $subtle, 168, 100)
$g.DrawString('1.24 GiB', $mid, $cyan, 20, 112)
$g.DrawString('312.50 MiB', $mid, $amber, 168, 112)

# Bottom accent bar, as the live card renders.
$g.FillRectangle($cyan, 20, 150, 120, 4)
$g.FillRectangle($amber, 160, 150, 120, 4)

$g.ResetClip()
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)

$path.Dispose()
foreach ($b in @($bg, $fg, $subtle, $cyan, $amber)) { $b.Dispose() }
foreach ($f in @($big, $mid, $small, $caption)) { $f.Dispose() }
$g.Dispose(); $bmp.Dispose()

$check = [System.Drawing.Image]::FromFile($out)
"wrote $out -> $($check.Width)x$($check.Height)"
$check.Dispose()
