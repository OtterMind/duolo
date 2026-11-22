// duolo-dictate：用 macOS 自带的语音识别（Speech 框架）录音并转写，随 Duolo.app 一起打包，
// 不需要另外安装或配置。遵循 Duolo 的语音命令约定（见 crates/duolo-core/src/config.rs 的 VoiceConfig）：
//
// - 启动即开始录音；
// - 识别过程中，每次有新结果就输出一行 `~ <目前听到的内容>`，界面用来实时显示；
// - 录音中大约每 60 毫秒输出一行 `^ <音量 0~1>`，界面用来画波形、发现麦克风没有声音；
// - 收到 SIGINT 后停止录音，等最终结果，输出一行 `= <最终结果>` 并以 0 退出；
// - 出错时把原因写到 stderr，以非 0 退出（2 表示没有权限）。
//
// 用法：duolo-dictate [--locale zh-CN]

import AVFoundation
import Foundation
import Speech

setvbuf(stdout, nil, _IOLBF, 0)

func fail(_ message: String, code: Int32 = 1) -> Never {
  FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
  exit(code)
}

var localeId = Locale.preferredLanguages.first ?? "en-US"
var args = CommandLine.arguments.dropFirst()
while let arg = args.popFirst() {
  if arg == "--locale", let value = args.popFirst() { localeId = value }
}

// 申请语音识别权限（麦克风权限在开始录音时由系统申请）。
let authorized = DispatchSemaphore(value: 0)
var speechStatus = SFSpeechRecognizerAuthorizationStatus.notDetermined
SFSpeechRecognizer.requestAuthorization { status in
  speechStatus = status
  authorized.signal()
}
authorized.wait()
guard speechStatus == .authorized else {
  fail("speech recognition permission denied", code: 2)
}

guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: localeId)) ?? SFSpeechRecognizer(),
  recognizer.isAvailable
else {
  fail("speech recognition is not available for \(localeId)")
}

let request = SFSpeechAudioBufferRecognitionRequest()
request.shouldReportPartialResults = true
if #available(macOS 13, *) {
  request.addsPunctuation = true
}
if recognizer.supportsOnDeviceRecognition {
  // 能在本机识别就不上传音频。
  request.requiresOnDeviceRecognition = true
}

let engine = AVAudioEngine()
let input = engine.inputNode
let format = input.outputFormat(forBus: 0)
guard format.sampleRate > 0 else { fail("no microphone input", code: 2) }
var lastLevelAt = Date.distantPast
input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, _ in
  request.append(buffer)
  let now = Date()
  guard now.timeIntervalSince(lastLevelAt) >= 0.06, let samples = buffer.floatChannelData?[0] else { return }
  lastLevelAt = now
  let count = Int(buffer.frameLength)
  guard count > 0 else { return }
  var sum: Float = 0
  for i in 0..<count { sum += samples[i] * samples[i] }
  let rms = (sum / Float(count)).squareRoot()
  // -50 dB 及以下算静音，0 dB 最大。
  let db = 20 * log10(max(rms, 1e-7))
  let level = min(max((db + 50) / 50, 0), 1)
  print(String(format: "^ %.2f", level))
}

var latest = ""
var finished = false
let done = DispatchSemaphore(value: 0)

let task = recognizer.recognitionTask(with: request) { result, error in
  if let result = result {
    latest = result.bestTranscription.formattedString
    if result.isFinal {
      finished = true
      done.signal()
    } else {
      print("~ \(latest)")
    }
  }
  if error != nil && !finished {
    finished = true
    done.signal()
  }
}

do {
  engine.prepare()
  try engine.start()
} catch {
  fail("cannot start recording: \(error.localizedDescription)", code: 2)
}

// SIGINT：停止录音，等最终结果。
signal(SIGINT, SIG_IGN)
let interrupt = DispatchSource.makeSignalSource(signal: SIGINT, queue: .global())
interrupt.setEventHandler {
  engine.stop()
  input.removeTap(onBus: 0)
  request.endAudio()
  // 最多再等 4 秒拿最终结果，拿不到就用目前听到的内容。
  _ = done.wait(timeout: .now() + 4)
  task.cancel()
  print("= \(latest)")
  exit(0)
}
interrupt.resume()

dispatchMain()
