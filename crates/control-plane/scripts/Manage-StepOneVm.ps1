# Fixed management protocol. Never accepts script text, a host command, or a workspace path.
# Windows PowerShell 5.1 compatible. Importing with dot-source only exposes functions for tests.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Stop-StepOne([string] $Code) {
    throw [InvalidOperationException]::new($Code)
}

function New-StepOneResponse([bool] $Ok, [bool] $Available, [string] $State, [bool] $Owned, [string[]] $Checks, $ErrorCode = $null) {
    return [ordered]@{ ok = $Ok; available = $Available; state = $State; owned = $Owned; checks = @($Checks); errorCode = $ErrorCode }
}

function Assert-StepOneRequest($Request) {
    if ($null -eq $Request -or $Request -isnot [pscustomobject]) { Stop-StepOne 'INVALID_REQUEST' }
    $names = @('action', 'resourceId', 'imagePath', 'imageSha256', 'baseRoot', 'cpuCount', 'memoryBytes', 'diskBytes', 'timeoutSeconds')
    if (@($Request.PSObject.Properties).Count -ne $names.Count) { Stop-StepOne 'INVALID_REQUEST' }
    foreach ($property in $Request.PSObject.Properties) {
        if ($names -cnotcontains $property.Name) { Stop-StepOne 'INVALID_REQUEST' }
    }
    if ($Request.action -isnot [string] -or @('probe', 'inspect', 'create', 'start', 'stop', 'reset', 'delete') -cnotcontains $Request.action) {
        Stop-StepOne 'INVALID_REQUEST'
    }
    foreach ($property in @('imagePath', 'imageSha256', 'baseRoot')) {
        if ($Request.$property -isnot [string] -or [string]::IsNullOrWhiteSpace($Request.$property) -or $Request.$property.Contains([char]0)) {
            Stop-StepOne 'INVALID_REQUEST'
        }
    }
    if ($Request.imageSha256 -cnotmatch '^[a-fA-F0-9]{64}$') { Stop-StepOne 'INVALID_REQUEST' }
    foreach ($property in @('cpuCount', 'memoryBytes', 'diskBytes', 'timeoutSeconds')) {
        if ($Request.$property -isnot [int] -and $Request.$property -isnot [long]) { Stop-StepOne 'INVALID_REQUEST' }
    }
    if ($Request.cpuCount -ne 2 -or $Request.memoryBytes -ne 2147483648 -or $Request.diskBytes -ne 34359738368 -or $Request.timeoutSeconds -ne 120) {
        Stop-StepOne 'INVALID_REQUEST'
    }
    $id = [guid]::Empty
    if ($Request.action -ceq 'probe') {
        if ($null -ne $Request.resourceId) { Stop-StepOne 'INVALID_REQUEST' }
    } elseif ($Request.resourceId -isnot [string] -or -not [guid]::TryParseExact($Request.resourceId, 'D', [ref]$id) -or $id -eq [guid]::Empty) {
        Stop-StepOne 'INVALID_REQUEST'
    }
    Assert-StepOneLocalPath $Request.baseRoot
    Assert-StepOneLocalPath $Request.imagePath
    $expected = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'com.ztkk.stepone\hyperv'
    if ([IO.Path]::GetFullPath($Request.baseRoot).TrimEnd('\') -ine [IO.Path]::GetFullPath($expected).TrimEnd('\')) {
        Stop-StepOne 'OWNERSHIP_DENIED'
    }
    if ([IO.Path]::GetExtension($Request.imagePath) -ine '.vhdx' -or (Test-StepOneInside $Request.imagePath $Request.baseRoot)) {
        Stop-StepOne 'IMAGE_INVALID'
    }
}

function Assert-StepOneLocalPath([string] $Path) {
    if ($Path -notmatch '^[A-Za-z]:[\\/]' -or $Path -match '(^|[\\/])\.\.?([\\/]|$)' -or $Path.Substring(2).Contains(':')) {
        Stop-StepOne 'INVALID_REQUEST'
    }
    $full = [IO.Path]::GetFullPath($Path)
    if ($full.TrimEnd('\') -ieq [IO.Path]::GetPathRoot($full).TrimEnd('\')) { Stop-StepOne 'OWNERSHIP_DENIED' }
    $cursor = $full
    while (-not [string]::IsNullOrEmpty($cursor)) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { Stop-StepOne 'OWNERSHIP_DENIED' }
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
}

function Test-StepOneInside([string] $Path, [string] $Root) {
    $pathFull = [IO.Path]::GetFullPath($Path).TrimEnd('\')
    $rootFull = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    return $pathFull.Equals($rootFull, [StringComparison]::OrdinalIgnoreCase) -or $pathFull.StartsWith($rootFull + '\', [StringComparison]::OrdinalIgnoreCase)
}

function Get-StepOneOwner([string] $BaseRoot, [string] $ResourceId) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes([IO.Path]::GetFullPath($BaseRoot).TrimEnd('\').ToLowerInvariant())
        $hash = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally { $sha.Dispose() }
    return 'step-one:v1:' + $ResourceId.ToLowerInvariant() + ':' + $hash
}

function Assert-StepOnePrerequisites {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { Stop-StepOne 'PREREQUISITE_MISSING' }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { Stop-StepOne 'PREREQUISITE_MISSING' }
    $systemDirectory = [Environment]::GetFolderPath('System')
    $modulePath = Join-Path $systemDirectory 'WindowsPowerShell\v1.0\Modules\Hyper-V\Hyper-V.psd1'
    if (-not (Test-Path -LiteralPath $modulePath -PathType Leaf) -or $null -eq (Get-Module -ListAvailable -Name $modulePath)) { Stop-StepOne 'PREREQUISITE_MISSING' }
    $service = Get-Service -Name vmms -ErrorAction SilentlyContinue
    if ($null -eq $service -or $service.Status -ne 'Running') { Stop-StepOne 'PREREQUISITE_MISSING' }
    # Never search a user-controlled PSModulePath for an elevated management module.
    Import-Module -Name $modulePath -ErrorAction Stop
}

function Write-StepOneMarker([string] $Path, [string] $Owner) {
    # CreateNew prevents replacing an unrelated marker; content contains no credentials.
    $bytes = [Text.Encoding]::UTF8.GetBytes((@{ owner = $Owner; schemaVersion = 1 } | ConvertTo-Json -Compress))
    $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
}

function Assert-StepOneMarker([string] $Path, [string] $Owner) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { Stop-StepOne 'OWNERSHIP_DENIED' }
    Assert-StepOneLocalPath $Path
    if ((Get-Item -LiteralPath $Path).Length -gt 4096) { Stop-StepOne 'OWNERSHIP_DENIED' }
    try { $marker = [IO.File]::ReadAllText($Path) | ConvertFrom-Json -ErrorAction Stop } catch { Stop-StepOne 'OWNERSHIP_DENIED' }
    if ($marker.schemaVersion -ne 1 -or $marker.owner -cne $Owner) { Stop-StepOne 'OWNERSHIP_DENIED' }
}

function Initialize-StepOneRoot([string] $Root, [bool] $Create) {
    Assert-StepOneLocalPath $Root
    $marker = Join-Path $Root '.step-one-root.json'
    if (Test-Path -LiteralPath $Root) {
        if (-not (Test-Path -LiteralPath $Root -PathType Container)) { Stop-StepOne 'OWNERSHIP_DENIED' }
        if (-not (Test-Path -LiteralPath $marker)) {
            if (-not $Create -or @(Get-ChildItem -LiteralPath $Root -Force).Count -ne 0) { Stop-StepOne 'OWNERSHIP_DENIED' }
            Write-StepOneMarker $marker 'step-one:hyperv-root:v1'
        }
    } elseif ($Create) {
        New-Item -ItemType Directory -Path $Root | Out-Null
        Write-StepOneMarker $marker 'step-one:hyperv-root:v1'
    } else { return $false }
    Assert-StepOneMarker $marker 'step-one:hyperv-root:v1'
    return $true
}

function Assert-StepOneImage($Request) {
    if (-not (Test-Path -LiteralPath $Request.imagePath -PathType Leaf)) { Stop-StepOne 'IMAGE_INVALID' }
    Assert-StepOneLocalPath $Request.imagePath
    if ((Get-FileHash -LiteralPath $Request.imagePath -Algorithm SHA256).Hash -ine $Request.imageSha256) { Stop-StepOne 'IMAGE_INVALID' }
    $image = Get-VHD -Path $Request.imagePath -ErrorAction Stop
    if ($image.VhdFormat -ne 'VHDX' -or $image.VhdType -eq 'Differencing' -or $image.Size -ne $Request.diskBytes -or $image.Attached) {
        Stop-StepOne 'IMAGE_INVALID'
    }
}

function Get-StepOneVm([string] $Name, [string] $Owner, [string] $ResourceRoot) {
    $matches = @(Get-VM -ErrorAction Stop | Where-Object { $null -ne $_ -and $_.Name -ieq $Name })
    if ($matches.Count -gt 1) { Stop-StepOne 'OWNERSHIP_DENIED' }
    if ($matches.Count -eq 0) { return $null }
    $vm = $matches[0]
    if ($vm.Name -cne $Name -or $vm.Notes -cne $Owner -or $vm.Generation -ne 2 -or -not (Test-StepOneInside $vm.ConfigurationLocation $ResourceRoot)) {
        Stop-StepOne 'OWNERSHIP_DENIED'
    }
    Assert-StepOneLocalPath $vm.ConfigurationLocation
    foreach ($disk in @(Get-VMHardDiskDrive -VM $vm -ErrorAction Stop)) {
        if ([IO.Path]::GetFullPath($disk.Path) -ine [IO.Path]::GetFullPath((Join-Path $ResourceRoot 'session.vhdx'))) { Stop-StepOne 'OWNERSHIP_DENIED' }
        Assert-StepOneLocalPath $disk.Path
    }
    return $vm
}

function Wait-StepOneVm($Vm, [string] $State, [int] $TimeoutSeconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $current = Get-VM -Id $Vm.Id -ErrorAction Stop
        if ($current.State.ToString() -eq $State) { return $current }
        if ([DateTime]::UtcNow -ge $deadline) { Stop-StepOne 'TIMEOUT' }
        Start-Sleep -Milliseconds 200
    } while ($true)
}

function Get-StepOneState($Vm) {
    if ($null -eq $Vm) { return 'missing' }
    switch ($Vm.State.ToString()) {
        'Running' { return 'running' }
        'Off' { return 'stopped' }
        default { Stop-StepOne 'OPERATION_FAILED' }
    }
}

function Assert-StepOneStoppedDisk($Vm, [string] $Disk) {
    if ($Vm.State.ToString() -ne 'Off') { Stop-StepOne 'OPERATION_FAILED' }
    Assert-StepOneLocalPath $Disk
    if (-not (Test-Path -LiteralPath $Disk -PathType Leaf)) { Stop-StepOne 'OWNERSHIP_DENIED' }
    $vhd = Get-VHD -Path $Disk -ErrorAction Stop
    if ($vhd.Attached -or $vhd.VhdType -ne 'Differencing') { Stop-StepOne 'OWNERSHIP_DENIED' }
}

function Invoke-StepOneRequest($Request) {
    Assert-StepOneRequest $Request
    Assert-StepOnePrerequisites
    if ($Request.action -ceq 'probe') {
        $checks = @('hyperv-module', 'vmms-running', 'management-permission')
        # Lost or unconfigured base images must not prevent stopping or deleting
        # an existing resource. Only create/start/reset require image readiness.
        if (Test-Path -LiteralPath $Request.imagePath -PathType Leaf) { $checks += 'image-present-unverified' }
        else { $checks += 'image-missing' }
        return New-StepOneResponse $true $true 'missing' $false $checks
    }
    $resourceId = ([guid]$Request.resourceId).ToString('D')
    $name = 'step-one-' + $resourceId
    $root = [IO.Path]::GetFullPath($Request.baseRoot)
    $resourceRoot = Join-Path $root $name
    $owner = Get-StepOneOwner $root $resourceId
    $marker = Join-Path $resourceRoot 'owner.json'
    $disk = Join-Path $resourceRoot 'session.vhdx'
    $exists = Initialize-StepOneRoot $root ($Request.action -ceq 'create')
    $vm = Get-StepOneVm $name $owner $resourceRoot
    if (-not $exists) {
        if ($null -ne $vm) { Stop-StepOne 'OWNERSHIP_DENIED' }
        if (@('inspect', 'stop', 'delete') -contains $Request.action) {
            return New-StepOneResponse $true $true 'missing' $false @('resource-absent')
        }
        Stop-StepOne 'OWNERSHIP_DENIED'
    }
    if (Test-Path -LiteralPath $resourceRoot) {
        Assert-StepOneLocalPath $resourceRoot
        Assert-StepOneMarker $marker $owner
    } elseif ($Request.action -ceq 'create' -and $null -eq $vm) {
        New-Item -ItemType Directory -Path $resourceRoot | Out-Null
        Write-StepOneMarker $marker $owner
    } elseif ($null -ne $vm) { Stop-StepOne 'OWNERSHIP_DENIED' }
    elseif (@('inspect', 'stop', 'delete') -contains $Request.action) {
        return New-StepOneResponse $true $true 'missing' $false @('resource-absent')
    } else { Stop-StepOne 'OWNERSHIP_DENIED' }

    switch ($Request.action) {
        'inspect' {
            return New-StepOneResponse $true $true (Get-StepOneState $vm) $true @('ownership-verified')
        }
        'create' {
            Assert-StepOneImage $Request
            if ($null -ne $vm) { return New-StepOneResponse $true $true (Get-StepOneState $vm) $true @('ownership-verified') }
            if (Test-Path -LiteralPath $disk) { Stop-StepOne 'CLEANUP_FAILED' }
            New-VHD -Path $disk -ParentPath $Request.imagePath -Differencing -ErrorAction Stop | Out-Null
            # If creation is interrupted before Notes is persisted, later operations
            # fail closed; an administrator must inspect that unclaimed VM.
            $vm = New-VM -Name $name -Generation 2 -MemoryStartupBytes $Request.memoryBytes -VHDPath $disk -Path $resourceRoot -ErrorAction Stop
            Set-VM -VM $vm -Notes $owner -AutomaticStartAction Nothing -AutomaticStopAction TurnOff -AutomaticCheckpointsEnabled $false -CheckpointType Disabled -ErrorAction Stop | Out-Null
            Set-VMProcessor -VM $vm -Count $Request.cpuCount -ErrorAction Stop | Out-Null
            Set-VMMemory -VM $vm -DynamicMemoryEnabled $false -StartupBytes $Request.memoryBytes -ErrorAction Stop | Out-Null
            Get-VMNetworkAdapter -VM $vm -ErrorAction Stop | Remove-VMNetworkAdapter -ErrorAction Stop
            Set-VMFirmware -VM $vm -EnableSecureBoot On -SecureBootTemplate MicrosoftUEFICertificateAuthority -ErrorAction Stop | Out-Null
            $vm = Get-StepOneVm $name $owner $resourceRoot
            if (@(Get-VMNetworkAdapter -VM $vm -ErrorAction Stop).Count -ne 0) { Stop-StepOne 'OPERATION_FAILED' }
            return New-StepOneResponse $true $true 'ready' $true @('ownership-verified', 'generation-2', 'network-disconnected', 'resource-limits', 'secure-boot')
        }
        'start' {
            if ($null -eq $vm) { Stop-StepOne 'OPERATION_FAILED' }
            Assert-StepOneImage $Request
            # Recheck that a host administrator did not add a network path or relax
            # CPU/memory policy after creation. Never fix foreign configuration.
            if (@(Get-VMNetworkAdapter -VM $vm -ErrorAction Stop).Count -ne 0 -or $vm.ProcessorCount -ne 2 -or $vm.MemoryStartup -ne 2147483648 -or $vm.DynamicMemoryEnabled) {
                Stop-StepOne 'OWNERSHIP_DENIED'
            }
            $firmware = Get-VMFirmware -VM $vm -ErrorAction Stop
            if ($firmware.SecureBoot -ne 'On' -or $firmware.SecureBootTemplate -ne 'MicrosoftUEFICertificateAuthority') { Stop-StepOne 'OWNERSHIP_DENIED' }
            $drives = @(Get-VMHardDiskDrive -VM $vm -ErrorAction Stop)
            if ($drives.Count -ne 1) { Stop-StepOne 'OWNERSHIP_DENIED' }
            Assert-StepOneLocalPath $disk
            $session = Get-VHD -Path $disk -ErrorAction Stop
            if ($session.VhdType -ne 'Differencing' -or $session.Size -ne $Request.diskBytes -or [IO.Path]::GetFullPath($session.ParentPath) -ine [IO.Path]::GetFullPath($Request.imagePath)) {
                Stop-StepOne 'OWNERSHIP_DENIED'
            }
            if ($vm.State.ToString() -ne 'Running') {
                Start-VM -VM $vm -ErrorAction Stop | Out-Null
                $vm = Wait-StepOneVm $vm 'Running' $Request.timeoutSeconds
            }
            return New-StepOneResponse $true $true 'running' $true @('ownership-verified', 'network-disconnected')
        }
        'stop' {
            if ($null -eq $vm) { return New-StepOneResponse $true $true 'missing' $true @('ownership-verified') }
            if ($vm.State.ToString() -ne 'Off') {
                Stop-VM -VM $vm -TurnOff -Force -ErrorAction Stop | Out-Null
                $vm = Wait-StepOneVm $vm 'Off' $Request.timeoutSeconds
            }
            return New-StepOneResponse $true $true 'stopped' $true @('ownership-verified')
        }
        'reset' {
            if ($null -eq $vm) { Stop-StepOne 'OPERATION_FAILED' }
            Assert-StepOneImage $Request
            Assert-StepOneStoppedDisk $vm $disk
            # Preserve the VM and ownership marker if disk replacement fails.
            $drives = @(Get-VMHardDiskDrive -VM $vm -ErrorAction Stop)
            if ($drives.Count -ne 1) { Stop-StepOne 'OWNERSHIP_DENIED' }
            Remove-VMHardDiskDrive -VMHardDiskDrive $drives[0] -ErrorAction Stop
            Remove-Item -LiteralPath $disk -Force
            New-VHD -Path $disk -ParentPath $Request.imagePath -Differencing -ErrorAction Stop | Out-Null
            Add-VMHardDiskDrive -VM $vm -Path $disk -ControllerType SCSI -ErrorAction Stop | Out-Null
            return New-StepOneResponse $true $true 'ready' $true @('ownership-verified', 'session-disk-recreated')
        }
        'delete' {
            if ($null -ne $vm) {
                if ($vm.State.ToString() -ne 'Off') { Stop-StepOne 'OPERATION_FAILED' }
                if (Test-Path -LiteralPath $disk) { Assert-StepOneStoppedDisk $vm $disk }
                Remove-VM -VM $vm -Force -ErrorAction Stop
            }
            if (Test-Path -LiteralPath $disk) {
                Assert-StepOneLocalPath $disk
                $vhd = Get-VHD -Path $disk -ErrorAction Stop
                if ($vhd.Attached -or $vhd.VhdType -ne 'Differencing') { Stop-StepOne 'OWNERSHIP_DENIED' }
                Remove-Item -LiteralPath $disk -Force
            }
            # Hyper-V may leave empty configuration directories. Remove only empty
            # directories, never recurse-delete a directory that could hold data.
            Remove-StepOneEmptyDirectories $resourceRoot
            if (@(Get-ChildItem -LiteralPath $resourceRoot -Force | Where-Object { $_.Name -cne 'owner.json' }).Count -ne 0) { Stop-StepOne 'CLEANUP_FAILED' }
            Remove-Item -LiteralPath $marker -Force
            Remove-Item -LiteralPath $resourceRoot -Force
            return New-StepOneResponse $true $true 'missing' $false @('owned-resources-removed')
        }
    }
}

function Remove-StepOneEmptyDirectories([string] $Directory, [int] $Depth = 0) {
    if ($Depth -gt 8) { Stop-StepOne 'CLEANUP_FAILED' }
    Assert-StepOneLocalPath $Directory
    foreach ($child in @(Get-ChildItem -LiteralPath $Directory -Directory -Force)) {
        # Validate before enumeration, so junctions are never traversed.
        Assert-StepOneLocalPath $child.FullName
        Remove-StepOneEmptyDirectories $child.FullName ($Depth + 1)
        if (@(Get-ChildItem -LiteralPath $child.FullName -Force).Count -eq 0) { Remove-Item -LiteralPath $child.FullName -Force }
    }
}

function Invoke-StepOneStdin {
    try {
        # The host assigns the helper to its kill-on-close Job before supplying
        # stdin. No module import or external operation happens before this read.
        $raw = [Console]::In.ReadToEnd()
        if ($raw.Length -eq 0 -or $raw.Length -gt 65536) { Stop-StepOne 'INVALID_REQUEST' }
        try { $request = $raw | ConvertFrom-Json -ErrorAction Stop } catch { Stop-StepOne 'INVALID_REQUEST' }
        $response = Invoke-StepOneRequest $request
        [Console]::Out.WriteLine(($response | ConvertTo-Json -Compress -Depth 5))
        exit 0
    } catch {
        $code = $_.Exception.Message
        if (@('INVALID_REQUEST', 'PREREQUISITE_MISSING', 'IMAGE_INVALID', 'OWNERSHIP_DENIED', 'TIMEOUT', 'OPERATION_FAILED', 'CLEANUP_FAILED') -cnotcontains $code) { $code = 'OPERATION_FAILED' }
        $response = New-StepOneResponse $false $false 'missing' $false @() $code
        [Console]::Out.WriteLine(($response | ConvertTo-Json -Compress -Depth 5))
        exit 1
    }
}

if ($MyInvocation.InvocationName -ne '.') { Invoke-StepOneStdin }
