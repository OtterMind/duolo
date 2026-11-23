import Foundation
import SQLite3

/// SQLite 的薄封装，只有这里直接碰 C 接口。不是线程安全的，由 `LocalStore` 排队使用。
final class Database {
    struct Failure: Error, CustomStringConvertible {
        let code: Int32
        let description: String
    }

    enum Value {
        case int(Int64)
        case text(String)
        case null
    }

    struct Row {
        fileprivate let statement: OpaquePointer

        func int(_ i: Int32) -> Int64 { sqlite3_column_int64(statement, i) }
        func text(_ i: Int32) -> String { sqlite3_column_text(statement, i).map { String(cString: $0) } ?? "" }
        func optionalText(_ i: Int32) -> String? { sqlite3_column_type(statement, i) == SQLITE_NULL ? nil : text(i) }
    }

    private var handle: OpaquePointer?

    /// `path` 为 nil 时在内存里（测试用）。
    init(path: String?) throws {
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_FULLMUTEX
        let code = sqlite3_open_v2(path ?? ":memory:", &handle, flags, nil)
        guard code == SQLITE_OK else {
            defer { sqlite3_close(handle) }
            throw Failure(code: code, description: String(cString: sqlite3_errmsg(handle)))
        }
        sqlite3_busy_timeout(handle, 3000)
        try run("pragma journal_mode = wal")
        try run("pragma foreign_keys = on")
    }

    deinit { sqlite3_close(handle) }

    var version: Int64 {
        get { (try? query("pragma user_version") { $0.int(0) }.first) ?? 0 }
    }

    func run(_ sql: String, _ values: Value...) throws {
        _ = try execute(sql, values) { _ in () }
    }

    func query<T>(_ sql: String, _ values: Value..., map: (Row) throws -> T) throws -> [T] {
        try execute(sql, values, map)
    }

    /// 全部成功才算，中途失败就当没做过。
    func transaction<T>(_ body: () throws -> T) throws -> T {
        try run("begin immediate")
        do {
            let result = try body()
            try run("commit")
            return result
        } catch {
            try? run("rollback")
            throw error
        }
    }

    private func execute<T>(_ sql: String, _ values: [Value], _ map: (Row) throws -> T) throws -> [T] {
        var statement: OpaquePointer?
        try check(sqlite3_prepare_v2(handle, sql, -1, &statement, nil))
        defer { sqlite3_finalize(statement) }
        guard let statement else { return [] }
        let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
        for (i, value) in values.enumerated() {
            let index = Int32(i + 1)
            switch value {
            case .int(let v): try check(sqlite3_bind_int64(statement, index, v))
            case .text(let v): try check(sqlite3_bind_text(statement, index, v, -1, transient))
            case .null: try check(sqlite3_bind_null(statement, index))
            }
        }
        var rows: [T] = []
        while true {
            switch sqlite3_step(statement) {
            case SQLITE_ROW: rows.append(try map(Row(statement: statement)))
            case SQLITE_DONE: return rows
            case let code: throw Failure(code: code, description: String(cString: sqlite3_errmsg(handle)))
            }
        }
    }

    private func check(_ code: Int32) throws {
        guard code == SQLITE_OK else { throw Failure(code: code, description: String(cString: sqlite3_errmsg(handle))) }
    }
}
