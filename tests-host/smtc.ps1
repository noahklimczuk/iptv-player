# What Windows' media overlay knows about the apps playing, and its buttons, from outside
# them: the same GlobalSystemMediaTransportControlsSessionManager the overlay itself reads.
#
#   powershell -NoProfile -File tests-host/smtc.ps1 list
#   powershell -NoProfile -File tests-host/smtc.ps1 pause aurora
#
# `list` prints every session as JSON. `play`, `pause`, `next` and `previous` press that
# button on the first session whose app id contains the second argument, and print
# whether Windows says it was taken. Used by scenarios/media_keys.py.
param(
    [Parameter(Mandatory = $true)][ValidateSet('list', 'play', 'pause', 'next', 'previous')]
    [string]$Action,
    [string]$App = 'aurora'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$asTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
} | Select-Object -First 1

function Wait-Op($op, [Type]$type) {
    $task = $asTask.MakeGenericMethod($type).Invoke($null, @($op))
    [void]$task.Wait(10000)
    $task.Result
}

$null = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType = WindowsRuntime]
$manager = Wait-Op ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) `
    ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager])
$sessions = @($manager.GetSessions())

if ($Action -eq 'list') {
    $out = foreach ($s in $sessions) {
        $p = Wait-Op ($s.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])
        $i = $s.GetPlaybackInfo()
        $t = $s.GetTimelineProperties()
        [pscustomobject]@{
            app       = $s.SourceAppUserModelId
            title     = $p.Title
            subtitle  = $p.Subtitle
            thumbnail = $null -ne $p.Thumbnail
            status    = "$($i.PlaybackStatus)"
            next      = $i.Controls.IsNextEnabled
            previous  = $i.Controls.IsPreviousEnabled
            pause     = $i.Controls.IsPauseEnabled
            endSecs   = $t.EndTime.TotalSeconds
        }
    }
    ConvertTo-Json -InputObject @($out) -Compress
    exit 0
}

$session = $sessions | Where-Object { $_.SourceAppUserModelId -like "*$App*" } | Select-Object -First 1
if (-not $session) { Write-Output 'no-session'; exit 2 }
$op = switch ($Action) {
    'play' { $session.TryPlayAsync() }
    'pause' { $session.TryPauseAsync() }
    'next' { $session.TrySkipNextAsync() }
    'previous' { $session.TrySkipPreviousAsync() }
}
Write-Output (Wait-Op $op ([bool]))
