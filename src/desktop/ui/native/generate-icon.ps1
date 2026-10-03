$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$bitmap = [System.Drawing.Bitmap]::new(256, 256)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$drop = [System.Drawing.Drawing2D.GraphicsPath]::new()
$arrow = [System.Drawing.Pen]::new([System.Drawing.ColorTranslator]::FromHtml('#122b50'), 3.5)
$fill = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml('#8ab4f8'))
try {
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.TranslateTransform(28, 4)
    $graphics.ScaleTransform(5, 5)
    $drop.AddBezier(20, 2, 15, 10, 4, 21, 4, 30)
    $drop.AddBezier(4, 30, 4, 38.837, 11.163, 46, 20, 46)
    $drop.AddBezier(20, 46, 28.837, 46, 36, 38.837, 36, 30)
    $drop.AddBezier(36, 30, 36, 21, 25, 10, 20, 2)
    $drop.CloseFigure()
    $graphics.FillPath($fill, $drop)
    $arrow.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
    $arrow.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
    $arrow.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round
    $graphics.DrawLine($arrow, 20, 35, 20, 20)
    $graphics.DrawLines($arrow, [System.Drawing.PointF[]]@(
        [System.Drawing.PointF]::new(14, 26),
        [System.Drawing.PointF]::new(20, 20),
        [System.Drawing.PointF]::new(26, 26)
    ))
    $bitmap.Save((Join-Path $PSScriptRoot 'drip.png'), [System.Drawing.Imaging.ImageFormat]::Png)
} finally {
    $fill.Dispose()
    $arrow.Dispose()
    $drop.Dispose()
    $graphics.Dispose()
    $bitmap.Dispose()
}
