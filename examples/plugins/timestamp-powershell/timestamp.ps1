# One-shot Sevak plugin: the query arrives as the only argument.
param([string]$Query = "")

# Sevak reads stdout as UTF-8; PowerShell defaults to the OEM code page.
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

$Query = $Query.Trim()
$moment = [DateTimeOffset]::Now
$note = "now"
if ($Query -match '^\d{9,13}$') {
    $number = [int64]$Query
    if ($Query.Length -ge 12) {
        $moment = [DateTimeOffset]::FromUnixTimeMilliseconds($number)
        $note = "milliseconds since 1970"
    } else {
        $moment = [DateTimeOffset]::FromUnixTimeSeconds($number)
        $note = "seconds since 1970"
    }
}

$rows = @(
    @{ key = "iso";   label = "ISO 8601 (UTC)";  value = $moment.UtcDateTime.ToString("yyyy-MM-ddTHH:mm:ssZ") },
    @{ key = "local"; label = "Local time";      value = $moment.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss zzz") },
    @{ key = "unix";  label = "Unix seconds";    value = [string]$moment.ToUnixTimeSeconds() },
    @{ key = "unixms"; label = "Unix milliseconds"; value = [string]$moment.ToUnixTimeMilliseconds() }
)

$items = foreach ($row in $rows) {
    @{
        key      = $row.key
        title    = $row.value
        subtitle = "$($row.label) ($note); Enter to copy"
        icon     = @{ kind = "builtin"; name = "copy" }
        action   = @{ type = "copy_text"; text = $row.value }
    }
}

@{ items = @($items) } | ConvertTo-Json -Depth 5 -Compress
