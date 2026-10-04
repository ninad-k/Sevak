# Color tools for Sevak

Requires Node.js 22+ on PATH. Install this folder as a script plugin, reload
Sevak, and allow its Node command when prompted.

- `color #f5b52c`: copy HEX, RGB or HSL.
- `color rgb(245, 181, 44)`: convert integer RGB.
- `color hsl(41, 91%, 57%)`: convert comma-separated HSL.
- `color #fff on #111`: view or copy a contrast report.

Everything runs locally. No settings, network requests or data files. Supports
opaque #RGB/#RRGGBB and RGB/HSL; screen sampling, alpha and named colors are
not supported. Reports use the unrounded WCAG 2.x text contrast ratio for
AA/AAA thresholds. They check color contrast, not overall accessibility.

[Contrast standard](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)
· [Source and tests](https://github.com/ninad-k/Sevak)
