param(
    [string]$ServiceExe = (Join-Path $PSScriptRoot "..\backend\tauri\sidecar\chimera-service-x86_64-pc-windows-msvc.exe"),
    [string]$ProbeUrl = "https://www.gstatic.com/generate_204",
    [switch]$SkipTraffic
)

$ErrorActionPreference = "Stop"
$script:checks = [System.Collections.Generic.List[object]]::new()
$script:failed = $false

function Add-Check {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][bool]$Passed,
        [Parameter(Mandatory = $true)][string]$Detail
    )

    $script:checks.Add([pscustomobject]@{
            name   = $Name
            passed = $Passed
            detail = $Detail
        }) | Out-Null
    if (-not $Passed) {
        $script:failed = $true
    }
}

function Get-TopLevelYamlScalar {
    param(
        [Parameter(Mandatory = $true)][string[]]$Lines,
        [Parameter(Mandatory = $true)][string]$Key
    )

    $pattern = "^" + [regex]::Escape($Key) + "\s*:\s*(.*?)\s*$"
    foreach ($line in $Lines) {
        if ($line -notmatch $pattern) {
            continue
        }

        $value = $matches[1].Trim()
        if ($value.Length -eq 0) {
            return ""
        }

        if ($value.StartsWith('"')) {
            try {
                return ($value | ConvertFrom-Json)
            }
            catch {
                throw "failed to parse double-quoted YAML scalar '$Key'"
            }
        }

        if ($value.StartsWith("'") -and $value.EndsWith("'")) {
            return $value.Substring(1, $value.Length - 2).Replace("''", "'")
        }

        return (($value -replace "\s+#.*$", "").Trim())
    }

    return $null
}

function ConvertTo-NetworkPrefix {
    param([Parameter(Mandatory = $true)][string]$Prefix)

    $parts = $Prefix.Trim().Split('/', 2)
    if ($parts.Count -ne 2) {
        throw "invalid CIDR '$Prefix'"
    }

    $ip = [System.Net.IPAddress]::Parse($parts[0])
    $prefixLength = [int]$parts[1]
    $bytes = $ip.GetAddressBytes()
    $bitCount = $bytes.Length * 8
    if ($prefixLength -lt 0 -or $prefixLength -gt $bitCount) {
        throw "invalid CIDR prefix length '$Prefix'"
    }

    for ($i = 0; $i -lt $bytes.Length; $i++) {
        $remaining = $prefixLength - ($i * 8)
        if ($remaining -ge 8) {
            $mask = 255
        }
        elseif ($remaining -le 0) {
            $mask = 0
        }
        else {
            $mask = 256 - [math]::Pow(2, 8 - $remaining)
        }
        $bytes[$i] = $bytes[$i] -band [int]$mask
    }

    $network = [System.Net.IPAddress]::new([byte[]]$bytes).ToString()
    return "$network/$prefixLength"
}

function Test-LoopbackController {
    param([Parameter(Mandatory = $true)][string]$Controller)

    try {
        $uri = [System.Uri]::new("http://$Controller")
    }
    catch {
        return $false
    }

    if ($uri.Host -ieq "localhost") {
        return $true
    }

    $address = $null
    if (-not [System.Net.IPAddress]::TryParse($uri.Host, [ref]$address)) {
        return $false
    }
    return [System.Net.IPAddress]::IsLoopback($address)
}

function Invoke-DirectHttpGet {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [hashtable]$Headers = @{},
        [int]$TimeoutSec = 5
    )

    Add-Type -AssemblyName System.Net.Http
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseProxy = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds($TimeoutSec)
    try {
        foreach ($name in $Headers.Keys) {
            if (-not $client.DefaultRequestHeaders.TryAddWithoutValidation([string]$name, [string]$Headers[$name])) {
                throw "failed to set HTTP header '$name'"
            }
        }

        $response = $client.GetAsync($Uri).GetAwaiter().GetResult()
        try {
            [pscustomobject]@{
                status_code = [int]$response.StatusCode
                success     = $response.IsSuccessStatusCode
                content     = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            }
        }
        finally {
            $response.Dispose()
        }
    }
    finally {
        $client.Dispose()
        $handler.Dispose()
    }
}

function Finish-Smoke {
    param(
        [object]$ServiceStatus,
        [object]$Tun
    )

    [pscustomobject]@{
        schema_version = 1
        captured_at    = [DateTimeOffset]::UtcNow.ToString("o")
        passed         = -not $script:failed
        service        = $ServiceStatus
        tun            = $Tun
        checks         = $script:checks
    } | ConvertTo-Json -Depth 10

    if ($script:failed) {
        exit 1
    }
    exit 0
}

$serviceSummary = $null
$tunSummary = $null

try {
    $resolvedServiceExe = (Resolve-Path -LiteralPath $ServiceExe).Path
    Add-Check "service_binary" $true $resolvedServiceExe

    $statusText = (& $resolvedServiceExe status --json 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "service status command failed with exit code ${LASTEXITCODE}: $statusText"
    }
    $status = $statusText | ConvertFrom-Json

    $serviceSummary = [ordered]@{
        name    = $status.name
        version = $status.version
        status  = $status.status
    }

    $serviceRunning = $status.status -eq "running"
    Add-Check "service_running" $serviceRunning "status=$($status.status)"
    if (-not $serviceRunning) {
        throw "Chimera Service is not running"
    }

    if ($null -eq $status.server) {
        Add-Check "service_server_status" $false "running service returned no server payload"
        throw "running service returned no server payload"
    }
    Add-Check "service_server_status" $true "server payload present"

    $serverVersion = [string]$status.server.version
    $parsedVersion = $null
    $versionParsed = [System.Version]::TryParse(($serverVersion -split '-', 2)[0], [ref]$parsedVersion)
    $compatible = $versionParsed -and $parsedVersion.Major -eq 1
    Add-Check "service_compatible" $compatible "server_version=$serverVersion required_major=1"
    if (-not $compatible) {
        throw "service protocol version is incompatible"
    }

    $coreState = $status.server.core_infos.state
    $coreRunning = $coreState -eq "Running"
    Add-Check "service_core_running" $coreRunning "state=$coreState"
    if (-not $coreRunning) {
        throw "Service does not report a running core"
    }

    $configPath = [string]$status.server.core_infos.config_path
    $configExists = -not [string]::IsNullOrWhiteSpace($configPath) -and (Test-Path -LiteralPath $configPath -PathType Leaf)
    Add-Check "runtime_config_present" $configExists "config_path=$configPath"
    if (-not $configExists) {
        throw "Service core runtime config is missing"
    }

    $runtimeParent = Split-Path -Parent $configPath
    if ((Split-Path -Leaf $runtimeParent) -ieq "runtime") {
        $expectedConfigRoot = Split-Path -Parent $runtimeParent
    }
    else {
        $expectedConfigRoot = $runtimeParent
    }
    $serviceConfigRoot = [string]$status.server.runtime_infos.nyanpasu_config_dir
    $configOwned = [System.IO.Path]::GetFullPath($expectedConfigRoot).TrimEnd('\') -ieq [System.IO.Path]::GetFullPath($serviceConfigRoot).TrimEnd('\')
    Add-Check "service_config_ownership" $configOwned "runtime_config_root=$expectedConfigRoot service_config_root=$serviceConfigRoot"
    if (-not $configOwned) {
        throw "Service runtime config belongs to a different config root"
    }

    $yamlLines = Get-Content -LiteralPath $configPath
    $controller = Get-TopLevelYamlScalar -Lines $yamlLines -Key "external-controller"
    $secret = Get-TopLevelYamlScalar -Lines $yamlLines -Key "secret"

    $controllerPresent = -not [string]::IsNullOrWhiteSpace($controller)
    Add-Check "controller_present" $controllerPresent "controller=$controller"
    if (-not $controllerPresent) {
        throw "runtime config has no external-controller"
    }

    $controllerLoopback = Test-LoopbackController -Controller $controller
    Add-Check "controller_loopback" $controllerLoopback "controller=$controller"
    if (-not $controllerLoopback) {
        throw "refusing to send Core API credentials to a non-loopback controller"
    }

    $headers = @{}
    if (-not [string]::IsNullOrEmpty($secret)) {
        $headers.Authorization = "Bearer $secret"
    }

    $coreResponse = Invoke-DirectHttpGet -Uri "http://$controller/configs" -Headers $headers -TimeoutSec 5
    if (-not $coreResponse.success) {
        throw "Core GET /configs returned HTTP $($coreResponse.status_code)"
    }
    $configs = $coreResponse.content | ConvertFrom-Json
    $tun = $configs.tun
    $tunPresent = $null -ne $tun
    Add-Check "core_tun_present" $tunPresent "GET /configs returned tun=$tunPresent"
    if (-not $tunPresent) {
        throw "Core /configs did not return a TUN section"
    }

    $tunEnabled = [bool]$tun.enable
    $device = [string]$tun.device
    $autoRouteProperty = $tun.PSObject.Properties['auto-route']
    $autoRouteKnown = $null -ne $autoRouteProperty
    $autoRoute = if ($autoRouteKnown) { [bool]$autoRouteProperty.Value } else { $null }

    $routeAddresses = [System.Collections.Generic.List[string]]::new()
    foreach ($field in @('route-address', 'inet4-route-address', 'inet6-route-address')) {
        $property = $tun.PSObject.Properties[$field]
        if ($null -eq $property -or $null -eq $property.Value) {
            continue
        }
        foreach ($route in @($property.Value)) {
            if (-not [string]::IsNullOrWhiteSpace([string]$route) -and -not $routeAddresses.Contains([string]$route)) {
                $routeAddresses.Add([string]$route)
            }
        }
    }

    $tunSummary = [ordered]@{
        enabled         = $tunEnabled
        device          = $device
        auto_route      = $autoRoute
        route_addresses = @($routeAddresses)
    }

    Add-Check "core_tun_enabled" $tunEnabled "tun.enable=$tunEnabled"
    if (-not $tunEnabled) {
        throw "Core TUN is not enabled"
    }

    $devicePresent = -not [string]::IsNullOrWhiteSpace($device)
    Add-Check "core_tun_device" $devicePresent "device=$device"
    if (-not $devicePresent) {
        throw "Core TUN device is empty"
    }

    $adapters = @(Get-NetAdapter -IncludeHidden -ErrorAction Stop | Where-Object { $_.Name -ieq $device })
    $adapterPresent = $adapters.Count -eq 1
    Add-Check "windows_tun_adapter" $adapterPresent "device=$device matches=$($adapters.Count)"
    if (-not $adapterPresent) {
        throw "Windows TUN adapter was not found by exact friendly-name match"
    }

    if (-not $autoRouteKnown) {
        Add-Check "core_tun_auto_route_known" $false "Core /configs omitted auto-route"
        throw "Core /configs omitted auto-route"
    }
    Add-Check "core_tun_auto_route_known" $true "auto-route=$autoRoute"

    if ($autoRoute) {
        $interfaces = @(Get-NetIPInterface -ErrorAction Stop | Where-Object { $_.InterfaceAlias -ieq $device })
        $indexes = @($interfaces | Select-Object -ExpandProperty InterfaceIndex -Unique)
        if ($indexes.Count -eq 0) {
            $indexes = @($adapters[0].ifIndex)
        }

        $observedRoutes = @(
            Get-NetRoute -ErrorAction Stop |
                Where-Object { $indexes -contains $_.InterfaceIndex } |
                ForEach-Object { ConvertTo-NetworkPrefix -Prefix $_.DestinationPrefix } |
                Select-Object -Unique
        )

        if ($routeAddresses.Count -eq 0) {
            $v4Default = $observedRoutes -contains "0.0.0.0/0"
            $v4Split = ($observedRoutes -contains "0.0.0.0/1") -and ($observedRoutes -contains "128.0.0.0/1")
            $v6Default = $observedRoutes -contains "::/0"
            $v6Split = ($observedRoutes -contains "::/1") -and ($observedRoutes -contains "8000::/1")
            $routeProof = $v4Default -or $v4Split -or $v6Default -or $v6Split
            Add-Check "windows_tun_routes" $routeProof "default_capture=$routeProof observed_count=$($observedRoutes.Count)"
        }
        else {
            $expectedRoutes = @($routeAddresses | ForEach-Object { ConvertTo-NetworkPrefix -Prefix $_ } | Select-Object -Unique)
            $missingRoutes = @($expectedRoutes | Where-Object { $observedRoutes -notcontains $_ })
            $routeProof = $missingRoutes.Count -eq 0
            Add-Check "windows_tun_routes" $routeProof "expected_count=$($expectedRoutes.Count) missing=$($missingRoutes -join ',')"
        }

        if (-not $routeProof) {
            throw "Windows routes do not prove Core TUN capture"
        }
    }
    else {
        Add-Check "windows_tun_routes" $true "auto-route=false; exact adapter presence is sufficient"
    }

    if (-not $SkipTraffic) {
        try {
            $traffic = Invoke-DirectHttpGet -Uri $ProbeUrl -TimeoutSec 8
            $trafficOk = $traffic.success -or ($traffic.status_code -ge 300 -and $traffic.status_code -lt 400)
            Add-Check "traffic_probe" $trafficOk "url=$ProbeUrl status=$($traffic.status_code) proxy=disabled"
            if (-not $trafficOk) {
                throw "traffic probe returned HTTP $($traffic.status_code)"
            }
        }
        catch {
            Add-Check "traffic_probe" $false "url=$ProbeUrl error=$($_.Exception.Message) proxy=disabled"
            throw
        }
    }
    else {
        Add-Check "traffic_probe" $true "skipped"
    }
}
catch {
    Add-Check "smoke_completed" $false $_.Exception.Message
    Finish-Smoke -ServiceStatus $serviceSummary -Tun $tunSummary
}

Add-Check "smoke_completed" $true "all requested read-only checks passed"
Finish-Smoke -ServiceStatus $serviceSummary -Tun $tunSummary
