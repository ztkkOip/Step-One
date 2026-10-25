# Contract tests use fake Hyper-V cmdlets and never import Hyper-V or change VMs.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'Manage-StepOneVm.ps1')
$script:Tests = 0

function Assert-True([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw ('Assertion failed: ' + $Message) }
}
function Assert-Error([scriptblock] $Action, [string] $Code) {
    try { & $Action | Out-Null } catch {
        Assert-True ($_.Exception.Message -ceq $Code) ('expected ' + $Code + ', got ' + $_.Exception.Message)
        return
    }
    throw ('Expected failure ' + $Code)
}
function Test-Case([string] $Name, [scriptblock] $Action) {
    & $Action
    $script:Tests += 1
    Write-Output ('PASS ' + $Name)
}
function Request([string] $Action = 'create') {
    return [pscustomobject]@{
        action = $Action; resourceId = 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee'
        imagePath = 'C:\StepOneTestImages\base.vhdx'; imageSha256 = ('a' * 64)
        baseRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'com.ztkk.stepone\hyperv'
        cpuCount = 2; memoryBytes = [long]2147483648; diskBytes = [long]34359738368; timeoutSeconds = 120
    }
}

Test-Case 'request accepts fixed schema and rejects arbitrary actions/fields' {
    Assert-StepOneRequest (Request)
    $bad = Request; $bad.action = 'shell'; Assert-Error { Assert-StepOneRequest $bad } 'INVALID_REQUEST'
    $bad = Request; $bad | Add-Member command 'Get-Process'; Assert-Error { Assert-StepOneRequest $bad } 'INVALID_REQUEST'
    $bad = Request; $bad.resourceId = 'not-a-uuid'; Assert-Error { Assert-StepOneRequest $bad } 'INVALID_REQUEST'
    $bad = Request; $bad.cpuCount = '2'; Assert-Error { Assert-StepOneRequest $bad } 'INVALID_REQUEST'
    $bad = Request; $bad.cpuCount = 8; Assert-Error { Assert-StepOneRequest $bad } 'INVALID_REQUEST'
    $probe = Request 'probe'; $probe.resourceId = $null; Assert-StepOneRequest $probe
}
Test-Case 'paths refuse roots, traversal, streams, unowned base, and image in private root' {
    Assert-Error { Assert-StepOneLocalPath 'C:\' } 'OWNERSHIP_DENIED'
    Assert-Error { Assert-StepOneLocalPath 'C:\safe\..\other' } 'INVALID_REQUEST'
    Assert-Error { Assert-StepOneLocalPath 'C:\safe\disk.vhdx:stream' } 'INVALID_REQUEST'
    Assert-Error { Assert-StepOneLocalPath '\\server\share\file' } 'INVALID_REQUEST'
    $bad = Request; $bad.baseRoot = 'C:\foreign'; Assert-Error { Assert-StepOneRequest $bad } 'OWNERSHIP_DENIED'
    $bad = Request; $bad.imagePath = Join-Path $bad.baseRoot 'base.vhdx'; Assert-Error { Assert-StepOneRequest $bad } 'IMAGE_INVALID'
}
Test-Case 'containment is component-scoped and owner stable per root/resource' {
    Assert-True (Test-StepOneInside 'C:\root\child' 'C:\root') 'child contained'
    Assert-True (-not (Test-StepOneInside 'C:\root-other' 'C:\root')) 'prefix sibling excluded'
    Assert-True ((Get-StepOneOwner 'C:\owned' 'AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE') -ceq (Get-StepOneOwner 'c:\OWNED\' 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee')) 'stable casing'
    Assert-True ((Get-StepOneOwner 'C:\owned' 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee') -cne (Get-StepOneOwner 'C:\different' 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee')) 'root-bound owner'
}

# Everything below is an in-memory cmdlet fixture. Never dot-source this test
# into an application or an administrator's interactive session.
$script:Request = Request
$script:ResourceRoot = Join-Path $script:Request.baseRoot ('step-one-' + $script:Request.resourceId)
$script:Disk = Join-Path $script:ResourceRoot 'session.vhdx'
$script:Name = 'step-one-' + $script:Request.resourceId
$script:Owner = Get-StepOneOwner $script:Request.baseRoot $script:Request.resourceId
$script:Vm = $null
$script:HasDisk = $false
$script:HasDirectory = $false
$script:ImagePresent = $true
$script:Nics = @()
$script:Calls = @()
$script:ForeignMarker = $false
$script:UnknownFile = $false

function Assert-StepOnePrerequisites { $script:Calls += 'prerequisites' }
function Assert-StepOneLocalPath([string] $Path) { }
function Initialize-StepOneRoot([string] $Root, [bool] $Create) { return $true }
function Assert-StepOneMarker([string] $Path, [string] $Owner) { if ($script:ForeignMarker) { Stop-StepOne 'OWNERSHIP_DENIED' } }
function Write-StepOneMarker([string] $Path, [string] $Owner) { $script:Calls += 'marker' }
function Assert-StepOneImage($Request) { if (-not $script:ImagePresent) { Stop-StepOne 'IMAGE_INVALID' } }
function Test-Path([string] $LiteralPath, [string] $PathType) {
    if ($LiteralPath -eq $script:Disk) { return $script:HasDisk }
    if ($LiteralPath -eq $script:ResourceRoot) { return $script:HasDirectory }
    if ($LiteralPath -eq $script:Request.imagePath) { return $script:ImagePresent }
    return $true
}
function Get-VM($Id, $ErrorAction) { return $script:Vm }
function Get-VMHardDiskDrive($VM, $ErrorAction) { if ($script:HasDisk) { return [pscustomobject]@{ Path = $script:Disk } } }
function Get-VHD($Path, $ErrorAction) { return [pscustomobject]@{ Attached = $false; VhdType = 'Differencing'; Size = [long]34359738368; ParentPath = $script:Request.imagePath } }
function New-Item($ItemType, $Path) { $script:HasDirectory = $true; $script:Calls += 'directory' }
function New-VHD($Path, $ParentPath, [switch] $Differencing, $ErrorAction) { $script:HasDisk = $true; $script:Calls += 'new-disk' }
function New-VM($Name, $Generation, $MemoryStartupBytes, $VHDPath, $Path, $ErrorAction) {
    Assert-True ($Generation -eq 2 -and $MemoryStartupBytes -eq 2147483648 -and $VHDPath -eq $script:Disk) 'VM arguments'
    $script:Vm = [pscustomobject]@{ Id = 'fixture-vm'; Name = $Name; Notes = ''; Generation = 2; ConfigurationLocation = $Path; State = 'Off'; ProcessorCount = 2; MemoryStartup = [long]2147483648; DynamicMemoryEnabled = $false }
    $script:Calls += 'new-vm'; return $script:Vm
}
function Set-VM($VM, $Notes, $AutomaticStartAction, $AutomaticStopAction, $AutomaticCheckpointsEnabled, $CheckpointType, $ErrorAction) {
    Assert-True ($AutomaticStartAction -eq 'Nothing' -and $AutomaticStopAction -eq 'TurnOff' -and $AutomaticCheckpointsEnabled -eq $false -and $CheckpointType -eq 'Disabled') 'auto policy'
    $script:Vm.Notes = $Notes; $script:Calls += 'vm-policy'
}
function Set-VMProcessor($VM, $Count, $ErrorAction) { Assert-True ($Count -eq 2) 'cpu limit'; $script:Calls += 'cpu' }
function Set-VMMemory($VM, $DynamicMemoryEnabled, $StartupBytes, $ErrorAction) { Assert-True ($DynamicMemoryEnabled -eq $false -and $StartupBytes -eq 2147483648) 'memory limit'; $script:Calls += 'memory' }
function Get-VMNetworkAdapter($VM, $ErrorAction) { return $script:Nics }
function Remove-VMNetworkAdapter($ErrorAction) { process { $script:Nics = @(); $script:Calls += 'remove-network' } }
function Set-VMFirmware($VM, $EnableSecureBoot, $SecureBootTemplate, $ErrorAction) { Assert-True ($EnableSecureBoot -eq 'On' -and $SecureBootTemplate -eq 'MicrosoftUEFICertificateAuthority') 'secure boot'; $script:Calls += 'firmware' }
function Get-VMFirmware($VM, $ErrorAction) { return [pscustomobject]@{ SecureBoot = 'On'; SecureBootTemplate = 'MicrosoftUEFICertificateAuthority' } }
function Start-VM($VM, $ErrorAction) { $script:Vm.State = 'Running'; $script:Calls += 'start' }
function Stop-VM($VM, [switch] $TurnOff, [switch] $Force, $ErrorAction) { $script:Vm.State = 'Off'; $script:Calls += 'stop' }
function Remove-VM($VM, [switch] $Force, $ErrorAction) { $script:Vm = $null; $script:Calls += 'delete-vm' }
function Remove-VMHardDiskDrive($VMHardDiskDrive, $ErrorAction) { $script:Calls += 'detach-disk' }
function Add-VMHardDiskDrive($VM, $Path, $ControllerType, $ErrorAction) { $script:Calls += 'attach-disk' }
function Remove-Item($LiteralPath, [switch] $Force) {
    Assert-True (Test-StepOneInside $LiteralPath $script:ResourceRoot) 'removal remains owned'
    if ($LiteralPath -eq $script:Disk) { $script:HasDisk = $false }
    if ($LiteralPath -eq $script:ResourceRoot) { $script:HasDirectory = $false }
    $script:Calls += 'remove-owned-item'
}
function Get-ChildItem($LiteralPath, [switch] $Directory, [switch] $Force) {
    if (-not $Directory) {
        [pscustomobject]@{ Name = 'owner.json' }
        if ($script:UnknownFile) { [pscustomobject]@{ Name = 'unrelated.txt' } }
    }
}

Test-Case 'create configures only owned Gen2 offline VM with fixed limits' {
    $result = Invoke-StepOneRequest (Request 'create')
    Assert-True ($result.ok -and $result.owned -and $result.state -eq 'ready') 'create response'
    foreach ($call in @('directory', 'marker', 'new-disk', 'new-vm', 'vm-policy', 'cpu', 'memory', 'firmware')) { Assert-True ($script:Calls -contains $call) $call }
}
Test-Case 'start-stop-reset lifecycle preserves scope' {
    Assert-True ((Invoke-StepOneRequest (Request 'start')).state -eq 'running') 'start'
    Assert-Error { Invoke-StepOneRequest (Request 'reset') } 'OPERATION_FAILED'
    Assert-True ((Invoke-StepOneRequest (Request 'stop')).state -eq 'stopped') 'stop'
    Assert-True ((Invoke-StepOneRequest (Request 'reset')).state -eq 'ready') 'reset'
    Assert-True ($script:Calls -contains 'detach-disk' -and $script:Calls -contains 'attach-disk') 'reset disk'
}
Test-Case 'foreign VM notes and network changes block start without mutation' {
    $script:Vm.Notes = 'foreign'; $before = @($script:Calls).Count
    Assert-Error { Invoke-StepOneRequest (Request 'delete') } 'OWNERSHIP_DENIED'
    Assert-True (@($script:Calls).Count -eq ($before + 1)) 'only prerequisite check'
    $script:Vm.Notes = $script:Owner
    $script:Nics = @([pscustomobject]@{ Name = 'external' })
    Assert-Error { Invoke-StepOneRequest (Request 'start') } 'OWNERSHIP_DENIED'
    $script:Nics = @()
}
Test-Case 'probe and cleanup remain possible when image disappears' {
    $script:ImagePresent = $false
    $probe = Request 'probe'; $probe.resourceId = $null
    Assert-True ((Invoke-StepOneRequest $probe).available) 'host stays available'
    Assert-True ((Invoke-StepOneRequest (Request 'inspect')).owned) 'inspect without image'
    Assert-True ((Invoke-StepOneRequest (Request 'stop')).state -eq 'stopped') 'stop without image'
    $script:UnknownFile = $true
    Assert-Error { Invoke-StepOneRequest (Request 'delete') } 'CLEANUP_FAILED'
    Assert-True ($script:HasDirectory) 'unknown content preserved'
    $script:UnknownFile = $false
    Assert-True ((Invoke-StepOneRequest (Request 'delete')).state -eq 'missing') 'retry owned cleanup'
    Assert-True (-not $script:HasDirectory -and -not $script:HasDisk -and $null -eq $script:Vm) 'resources removed'
}
Write-Output ('Passed ' + $script:Tests + ' safe management contract tests; no Hyper-V operations executed.')
