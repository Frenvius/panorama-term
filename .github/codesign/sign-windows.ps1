param([Parameter(Mandatory)][string]$File)

$ErrorActionPreference = 'Stop'

$Csp = 'SimplySign CSP'
$KeyContainer = '61CF1924D310EA4697F2DB4A22BC48F1DD0445B7'
$CertPath = Join-Path $PSScriptRoot 'certum-codesign.cer'
$TimestampUrl = 'http://time.certum.pl'
$SimplySignExe = 'C:\Program Files\Certum\SimplySign Desktop\SimplySignDesktop.exe'
$SignTimeoutSeconds = 180
$MaxLoginAttempts = 3

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

function Get-SignTool {
    $found = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $found) { throw 'signtool.exe not found' }
    $found.FullName
}

function ConvertFrom-Base32([string]$Text) {
    $alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567'
    $clean = $Text.ToUpperInvariant().TrimEnd('=').Replace(' ', '')
    $bits = 0
    $value = 0
    $bytes = [System.Collections.Generic.List[byte]]::new()
    foreach ($char in $clean.ToCharArray()) {
        $index = $alphabet.IndexOf($char)
        if ($index -lt 0) { throw 'Invalid base32 character in TOTP secret' }
        $value = ($value -shl 5) -bor $index
        $bits += 5
        if ($bits -ge 8) {
            $bits -= 8
            $bytes.Add([byte](($value -shr $bits) -band 0xFF))
            $value = $value -band ((1 -shl $bits) - 1)
        }
    }
    $bytes.ToArray()
}

function Get-OtpParameters([string]$Uri) {
    $query = @{}
    foreach ($pair in ([uri]$Uri).Query.TrimStart('?').Split('&')) {
        $parts = $pair.Split('=', 2)
        if ($parts.Count -eq 2) { $query[$parts[0].ToLowerInvariant()] = [uri]::UnescapeDataString($parts[1]) }
    }
    if (-not $query.secret) { throw 'SIMPLYSIGN_OTP_URI has no secret parameter' }
    [pscustomobject]@{
        Secret    = ConvertFrom-Base32 $query.secret
        Algorithm = if ($query.algorithm) { $query.algorithm.ToUpperInvariant() } else { 'SHA1' }
        Digits    = if ($query.digits) { [int]$query.digits } else { 6 }
        Period    = if ($query.period) { [int]$query.period } else { 30 }
    }
}

function New-Hmac([string]$Algorithm, [byte[]]$Key) {
    switch ($Algorithm) {
        'SHA1' { [System.Security.Cryptography.HMACSHA1]::new($Key) }
        'SHA256' { [System.Security.Cryptography.HMACSHA256]::new($Key) }
        'SHA512' { [System.Security.Cryptography.HMACSHA512]::new($Key) }
        default { throw "Unsupported TOTP algorithm: $Algorithm" }
    }
}

# SimplySign rejects a code that was already used, so every login waits for a fresh TOTP window.
function Get-FreshTotp($Otp) {
    $stateFile = Join-Path ($env:RUNNER_TEMP ?? $env:TEMP) 'simplysign-last-counter'
    $last = if (Test-Path $stateFile) { [long](Get-Content $stateFile) } else { -1 }
    $counter = [long][math]::Floor([DateTimeOffset]::UtcNow.ToUnixTimeSeconds() / $Otp.Period)
    while ($counter -le $last) {
        Start-Sleep -Seconds 1
        $counter = [long][math]::Floor([DateTimeOffset]::UtcNow.ToUnixTimeSeconds() / $Otp.Period)
    }
    Set-Content $stateFile $counter

    $message = [BitConverter]::GetBytes($counter)
    if ([BitConverter]::IsLittleEndian) { [array]::Reverse($message) }
    $hmac = New-Hmac $Otp.Algorithm $Otp.Secret
    $hash = $hmac.ComputeHash($message)
    $offset = $hash[$hash.Length - 1] -band 0x0F
    $binary = (([int]$hash[$offset] -band 0x7F) -shl 24) -bor ([int]$hash[$offset + 1] -shl 16) -bor ([int]$hash[$offset + 2] -shl 8) -bor [int]$hash[$offset + 3]
    ([long]$binary % [long][math]::Pow(10, $Otp.Digits)).ToString().PadLeft($Otp.Digits, '0')
}

function Find-ById($Parent, [string]$AutomationId) {
    $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty, $AutomationId)
    $Parent.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
}

function Find-LoginForm {
    $app = Get-Process SimplySignDesktop -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $app) { return $null }
    $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $app.Id)
    $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
    foreach ($window in $windows) {
        if (Find-ById $window 'tPassword') { return $window }
    }
    $null
}

function Submit-Login($Form, $Otp) {
    $user = Find-ById $Form 'tUserName'
    $password = Find-ById $Form 'tPassword'
    $ok = Find-ById $Form 'bOk'
    $user.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($env:SIMPLYSIGN_USER)
    $password.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue((Get-FreshTotp $Otp))
    $ok.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}

if (-not $env:SIMPLYSIGN_USER -or -not $env:SIMPLYSIGN_OTP_URI) { throw 'SIMPLYSIGN_USER and SIMPLYSIGN_OTP_URI must be set' }
$otp = Get-OtpParameters $env:SIMPLYSIGN_OTP_URI

if (-not (Get-Process SimplySignDesktop -ErrorAction SilentlyContinue)) {
    Start-Process $SimplySignExe
    Start-Sleep -Seconds 3
}

$logDir = Join-Path ($env:RUNNER_TEMP ?? $env:TEMP) 'simplysign-sign'
New-Item -ItemType Directory -Force $logDir | Out-Null
$stdout = Join-Path $logDir 'stdout.log'
$stderr = Join-Path $logDir 'stderr.log'

$arguments = @('sign', '/f', "`"$CertPath`"", '/csp', "`"$Csp`"", '/kc', $KeyContainer, '/fd', 'sha256', '/tr', $TimestampUrl, '/td', 'sha256', "`"$File`"")
$signer = Start-Process (Get-SignTool) -ArgumentList $arguments -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError $stderr

$deadline = (Get-Date).AddSeconds($SignTimeoutSeconds)
$attempts = 0
while (-not $signer.HasExited) {
    if ((Get-Date) -gt $deadline) {
        $signer.Kill()
        throw "Timed out signing $File"
    }
    $form = Find-LoginForm
    if ($form) {
        if ($attempts -ge $MaxLoginAttempts) {
            $signer.Kill()
            throw 'SimplySign login failed, check SIMPLYSIGN_USER and SIMPLYSIGN_OTP_URI'
        }
        $attempts++
        Write-Host "SimplySign login attempt $attempts"
        Submit-Login $form $otp
        Start-Sleep -Seconds 3
    }
    Start-Sleep -Milliseconds 500
}
$signer.WaitForExit()

Get-Content $stdout, $stderr -ErrorAction SilentlyContinue | Write-Host
if ($signer.ExitCode -ne 0) { throw "signtool failed with exit code $($signer.ExitCode) for $File" }

$signature = Get-AuthenticodeSignature $File
if ($signature.Status -ne 'Valid') { throw "Signature on $File is $($signature.Status)" }
Write-Host "Signed $File"
