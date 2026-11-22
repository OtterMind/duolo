# duolo-dictate（Windows 版）：用 Windows 自带的语音识别（System.Speech 听写）录音并转写，
# 随 Duolo 一起安装，不需要另外配置。输出约定和 macOS 版（dictate/main.swift）一样：
#
# - 启动即开始录音；
# - 识别过程中，每次有新结果就输出一行 `~ <目前听到的内容>`；
# - 录音中大约每 60 毫秒输出一行 `^ <音量 0~1>`；
# - 标准输入收到一行 `stop`（或被关闭）后停止录音，输出一行 `= <最终结果>` 并以 0 退出；
# - 出错时把原因写到 stderr，以非 0 退出（2 表示没有麦克风）。
#
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File dictate.ps1 -locale zh-CN
# 需要 Windows PowerShell 5（powershell.exe）：PowerShell 7 没有 System.Speech。

param([string]$locale = "")

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Add-Type -AssemblyName System.Speech

function Fail([string]$message, [int]$code = 1) {
    [Console]::Error.WriteLine($message)
    exit $code
}

# 选识别器：先找完全一致的语言（zh-CN），再找同一种语言（zh-*），都没有就用系统默认的。
$recognizers = [System.Speech.Recognition.SpeechRecognitionEngine]::InstalledRecognizers()
if ($recognizers.Count -eq 0) { Fail "no speech recognizer is installed" }
$info = $null
if ($locale) {
    $info = $recognizers | Where-Object { $_.Culture.Name -eq $locale } | Select-Object -First 1
    if (-not $info) {
        $language = $locale.Split("-")[0]
        $info = $recognizers | Where-Object { $_.Culture.TwoLetterISOLanguageName -eq $language } | Select-Object -First 1
    }
}
if (-not $info) { $info = $recognizers | Select-Object -First 1 }

$engine = New-Object System.Speech.Recognition.SpeechRecognitionEngine($info)
$engine.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar))
try {
    $engine.SetInputToDefaultAudioDevice()
} catch {
    Fail "no microphone input: $($_.Exception.Message)" 2
}

Register-ObjectEvent -InputObject $engine -EventName SpeechHypothesized -SourceIdentifier hyp | Out-Null
Register-ObjectEvent -InputObject $engine -EventName SpeechRecognized -SourceIdentifier rec | Out-Null
Register-ObjectEvent -InputObject $engine -EventName AudioLevelUpdated -SourceIdentifier lvl | Out-Null
Register-ObjectEvent -InputObject $engine -EventName RecognizeCompleted -SourceIdentifier done | Out-Null

# 已经确定的句子，加上正在识别的这一句，就是"目前听到的内容"。
$final = New-Object System.Collections.Generic.List[string]
$script:lastLevel = [DateTime]::MinValue
$script:completed = $false

function Heard([string]$current = "") {
    $parts = @($final) + @($current) | Where-Object { $_ }
    return ($parts -join " ").Trim()
}

function Drain {
    foreach ($ev in @(Get-Event)) {
        switch ($ev.SourceIdentifier) {
            "hyp" { [Console]::Out.WriteLine("~ " + (Heard $ev.SourceEventArgs.Result.Text)) }
            "rec" {
                $final.Add($ev.SourceEventArgs.Result.Text)
                [Console]::Out.WriteLine("~ " + (Heard))
            }
            "lvl" {
                $now = [DateTime]::UtcNow
                if (($now - $script:lastLevel).TotalMilliseconds -ge 60) {
                    $script:lastLevel = $now
                    $level = [Math]::Min(1.0, $ev.SourceEventArgs.AudioLevel / 100.0)
                    [Console]::Out.WriteLine("^ " + $level.ToString("0.00", [Globalization.CultureInfo]::InvariantCulture))
                }
            }
            "done" { $script:completed = $true }
        }
        Remove-Event -EventIdentifier $ev.EventIdentifier
    }
    [Console]::Out.Flush()
}

$engine.RecognizeAsync([System.Speech.Recognition.RecognizeMode]::Multiple)

# 一边处理识别事件，一边等标准输入的 `stop`。
$stop = [Console]::In.ReadLineAsync()
while (-not $stop.IsCompleted) {
    Drain
    Start-Sleep -Milliseconds 30
}

# 停止录音，最多再等 4 秒拿到最后一句。
$engine.RecognizeAsyncStop()
$deadline = [DateTime]::UtcNow.AddSeconds(4)
while (-not $script:completed -and [DateTime]::UtcNow -lt $deadline) {
    Drain
    Start-Sleep -Milliseconds 30
}
Drain
[Console]::Out.WriteLine("= " + (Heard))
[Console]::Out.Flush()
exit 0
