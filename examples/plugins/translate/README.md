# Translate in browser for Sevak

Requires Node.js 22+ on PATH. Install this folder as a script plugin, reload
Sevak, and allow its Node command when prompted.

- `tr fr hello world`: translate into French.
- `tr en bonjour`: translate into English.
- `tr hi good morning`: translate into Hindi.
- `tr `: show the supported target-language codes.

Enter opens Google Translate in your browser, with source-language detection.
This is a browser shortcut, not an in-launcher translation engine. There is
no API key or translation history. The plugin makes no network requests and
stores no text; Google receives the input only when you open the result.
Supports up to 5,000 Unicode characters. Internet access is needed by the browser.
