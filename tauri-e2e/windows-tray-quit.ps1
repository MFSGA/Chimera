param(
    [Parameter(Mandatory = $true)][int]$AppPid
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class TrayMouse {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
}
'@
$root = [System.Windows.Automation.AutomationElement]::RootElement
$scope = [System.Windows.Automation.TreeScope]::Descendants
$tooltip = "Chimera E2E $AppPid"

function Find-ByName([string]$name) {
    $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $name)
    return $root.FindFirst($scope, $condition)
}
function Click-Element($item, [bool]$right) {
    $point = $item.GetClickablePoint()
    if (-not [TrayMouse]::SetCursorPos([int]$point.X, [int]$point.Y)) {
        throw 'Could not position the native mouse'
    }
    if ($right) {
        [TrayMouse]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
        [TrayMouse]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
    } else {
        [TrayMouse]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        [TrayMouse]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    }
}
function Find-VisibleTrayIcon {
    $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $tooltip)
    foreach ($item in $root.FindAll($scope, $condition)) {
        try {
            if (-not $item.Current.IsOffscreen) {
                $null = $item.GetClickablePoint()
                return $item
            }
        } catch {}
    }
    return $null
}
$deadline = [DateTime]::UtcNow.AddSeconds(60)
$overflowOpened = $false
$icon = $null
while ([DateTime]::UtcNow -lt $deadline) {
    if (-not (Get-Process -Id $AppPid -ErrorAction SilentlyContinue)) {
        throw 'The test application exited before the tray interaction'
    }
    $icon = Find-VisibleTrayIcon
    if ($null -ne $icon) { break }
    if (-not $overflowOpened) {
        # Explorer uses different accessible names on Windows 10/11.
        $hiddenZh = -join ([char[]]@(0x663e, 0x793a, 0x9690, 0x85cf, 0x7684, 0x56fe, 0x6807))
        $hiddenZhShort = -join ([char[]]@(0x663e, 0x793a, 0x9690, 0x85cf, 0x56fe, 0x6807))
        foreach ($name in @('Show hidden icons', 'Hidden icons', $hiddenZh, $hiddenZhShort)) {
            $button = Find-ByName $name
            if ($null -ne $button -and -not $button.Current.IsOffscreen) {
                Click-Element $button $false
                $overflowOpened = $true
                break
            }
        }
    }
    Start-Sleep -Milliseconds 250
}
if ($null -eq $icon) { throw "Native tray icon not found: $tooltip" }
Click-Element $icon $true

# Only accept the real menu item belonging to this application's process.
$pidCondition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $AppPid)
$menuCondition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::MenuItem)
$condition = [System.Windows.Automation.AndCondition]::new([System.Windows.Automation.Condition[]]@($pidCondition, $menuCondition))
$quitZh = -join ([char[]]@(0x9000, 0x51fa))
$quitZhTraditional = -join ([char[]]@(0x7d50, 0x675f))
$deadline = [DateTime]::UtcNow.AddSeconds(10)
$quit = $null
while ([DateTime]::UtcNow -lt $deadline) {
    foreach ($item in $root.FindAll($scope, $condition)) {
        if ($item.Current.Name -in @('Quit', 'Exit', $quitZh, $quitZhTraditional) -and -not $item.Current.IsOffscreen) {
            $quit = $item
            break
        }
    }
    if ($null -ne $quit) { break }
    Start-Sleep -Milliseconds 100
}
if ($null -eq $quit) { throw 'The native Quit menu item was not found for the test process' }
Click-Element $quit $false
Write-Output 'Native tray Quit clicked'
