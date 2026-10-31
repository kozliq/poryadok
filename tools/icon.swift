// Рисует значок: визитка цвета кости, перламутровый отлив, буква «П» гарамондом.
// Запуск: swift tools/icon.swift ui/fonts/EBGaramond.ttf app/icons/source.png
import AppKit
import CoreText

let args = CommandLine.arguments
let fontURL = URL(fileURLWithPath: args[1]) as CFURL
CTFontManagerRegisterFontsForURL(fontURL, .process, nil)
let size: CGFloat = 1024
let ctx = CGContext(data: nil, width: Int(size), height: Int(size), bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!

// Квадрат со скруглением по сетке значков macOS: поле 824 из 1024, радиус ~22,5 %.
let inset: CGFloat = 100, side = size - 2 * inset
let card = CGPath(roundedRect: CGRect(x: inset, y: inset, width: side, height: side), cornerWidth: side * 0.225, cornerHeight: side * 0.225, transform: nil)
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -14), blur: 34, color: CGColor(red: 0.12, green: 0.1, blue: 0.08, alpha: 0.28))
ctx.addPath(card); ctx.setFillColor(CGColor(red: 0.918, green: 0.890, blue: 0.835, alpha: 1)); ctx.fillPath()
ctx.restoreGState()

// Перламутр: три мягких пятна — розовое, голубое, мятное.
ctx.saveGState(); ctx.addPath(card); ctx.clip()
let sRGB = CGColorSpace(name: CGColorSpace.sRGB)!
for (cx, cy, r, g, b) in [(0.28, 0.78, 0.98, 0.88, 0.90), (0.80, 0.62, 0.86, 0.91, 0.97), (0.55, 0.18, 0.89, 0.95, 0.90)] as [(CGFloat, CGFloat, CGFloat, CGFloat, CGFloat)] {
    let grad = CGGradient(colorsSpace: sRGB, colors: [CGColor(red: r, green: g, blue: b, alpha: 0.75), CGColor(red: r, green: g, blue: b, alpha: 0)] as CFArray, locations: [0, 1])!
    let c = CGPoint(x: inset + side * cx, y: inset + side * cy)
    ctx.drawRadialGradient(grad, startCenter: c, startRadius: 0, endCenter: c, endRadius: side * 0.55, options: [])
}
// Тонкая кромка, как у плотного картона.
ctx.addPath(card); ctx.setStrokeColor(CGColor(red: 0.12, green: 0.1, blue: 0.08, alpha: 0.10)); ctx.setLineWidth(3); ctx.strokePath()
ctx.restoreGState()

// Буква и линейка под ней.
let ink = CGColor(red: 0.118, green: 0.106, blue: 0.094, alpha: 1)
let font = CTFontCreateWithName("EB Garamond" as CFString, 560, nil)
let attr = NSAttributedString(string: "П", attributes: [.font: font, .foregroundColor: ink])
let line = CTLineCreateWithAttributedString(attr)
let bounds = CTLineGetBoundsWithOptions(line, .useGlyphPathBounds)
ctx.textPosition = CGPoint(x: (size - bounds.width) / 2 - bounds.minX, y: size * 0.535 - bounds.height / 2 - bounds.minY)
CTLineDraw(line, ctx)
ctx.setFillColor(ink)
ctx.fill(CGRect(x: size / 2 - 120, y: size * 0.285, width: 240, height: 7))

let img = ctx.makeImage()!
let rep = NSBitmapImageRep(cgImage: img)
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[2]))
print("ok")
