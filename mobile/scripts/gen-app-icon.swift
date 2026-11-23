// 生成 iOS 的应用图标：和桌面端同一个标志（crates/duolo-app/icons/icon.png），底铺满，不带透明。
//   swift mobile/scripts/gen-app-icon.swift mobile/ios/Duolo/Assets.xcassets/AppIcon.appiconset/icon.png
import AppKit

let size = 1024
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let ctx = CGContext(
    data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0, space: space, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue)!
func gray(_ v: CGFloat) -> CGColor { CGColor(colorSpace: space, components: [v, v, v, 1])! }

let gradient = CGGradient(colorsSpace: space, colors: [gray(0.17), gray(0.09)] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: size), end: .zero, options: [])

func circle(_ x: CGFloat, _ r: CGFloat) -> CGRect { CGRect(x: x - r, y: 512 - r, width: 2 * r, height: 2 * r) }
let scale: CGFloat = 1.12
let (left, right) = (512 - 60 * scale, 512 + 60 * scale)
ctx.setFillColor(gray(0.93))
ctx.fillEllipse(in: circle(right, 190 * scale))
ctx.saveGState()
ctx.addEllipse(in: circle(right, 146 * scale))
ctx.clip()
ctx.drawLinearGradient(gradient, start: CGPoint(x: 0, y: size), end: .zero, options: [])
ctx.restoreGState()
ctx.fillEllipse(in: circle(left, 190 * scale))

let rep = NSBitmapImageRep(cgImage: ctx.makeImage()!)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(filePath: CommandLine.arguments[1]))
