import Foundation

public enum Excerpt {
    /// 一段 Markdown 的第一句话，去掉格式。规则和 Android 一致，样例在 mobile/contract/behavior.json。
    public static func first(_ markdown: String) -> String {
        var inCode = false
        for rawLine in markdown.split(whereSeparator: \.isNewline) {
            var line = rawLine.trimmingCharacters(in: .whitespaces)
            if line.hasPrefix("```") {
                inCode.toggle()
                continue
            }
            if inCode || line.isEmpty { continue }
            while let marker = line.range(of: #"^(#{1,6}|>|[-*+]|\d+[.)])\s+"#, options: .regularExpression) {
                line.removeSubrange(marker)
            }
            line = line.replacingOccurrences(of: #"\[([^\]]+)\]\([^)]*\)"#, with: "$1", options: .regularExpression)
            for mark in ["**", "`", "~~"] { line = line.replacingOccurrences(of: mark, with: "") }
            if line.isEmpty { continue }
            return firstSentence(line)
        }
        return ""
    }

    private static func firstSentence(_ line: String) -> String {
        guard let end = line.range(of: #"[。！？]|[.!?](?=\s)"#, options: .regularExpression) else { return line }
        return String(line[..<end.upperBound])
    }
}
