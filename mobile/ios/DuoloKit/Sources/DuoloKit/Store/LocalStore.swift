import Foundation

/// 配对过的一台电脑。令牌和密钥在钥匙串里（`SecretStore`），不在数据库里。
public struct Computer: Hashable, Sendable, Identifiable {
    public let id: String
    public var name: String
    public var platform: String
    public let server: URL
    /// 这台手机在服务器上的设备编号。
    public let deviceId: String
    public let desktopKey: Data

    public init(id: String, name: String, platform: String, server: URL, deviceId: String, desktopKey: Data) {
        self.id = id
        self.name = name
        self.platform = platform
        self.server = server
        self.deviceId = deviceId
        self.desktopKey = desktopKey
    }
}

/// 还没有结果的命令，或者没成功的。成功的就删掉了。
public struct OutboxItem: Hashable, Sendable, Identifiable {
    public enum State: String, Sendable { case queued, sent, failed }

    public let id: String
    public let command: Command
    public let at: Int64
    public var state: State
    public var error: String?
}

/// 手机上保存的内容。所有读写排队进行；写入都在事务里，要么全部成功要么没做过。
public final class LocalStore: @unchecked Sendable {
    private let queue = DispatchQueue(label: "duolo.store")
    private let db: Database

    public init(path: String?) throws {
        db = try Database(path: path)
        try migrate()
    }

    /// 数据库的版本只往上加，每一步都能从上一步升级。
    private func migrate() throws {
        let steps: [[String]] = [
            [
                """
                create table computer (
                  id text primary key, name text not null, platform text not null, server text not null,
                  device_id text not null, desktop_key text not null, added_at integer not null)
                """,
                """
                create table overview (
                  computer text primary key references computer(id) on delete cascade, rev integer not null, json text not null)
                """,
                """
                create table event (
                  computer text not null references computer(id) on delete cascade,
                  task integer not null, seq integer not null, json text not null, primary key (computer, task, seq))
                """,
                """
                create table review (
                  computer text not null references computer(id) on delete cascade,
                  task integer not null, finished_at integer not null, output text not null, primary key (computer, task))
                """,
                """
                create table outbox (
                  id text primary key, computer text not null references computer(id) on delete cascade,
                  json text not null, at integer not null, state text not null, error text)
                """,
            ]
        ]
        for (i, statements) in steps.enumerated() where db.version <= Int64(i) {
            try db.transaction {
                for sql in statements { try db.run(sql) }
                try db.run("pragma user_version = \(i + 1)")
            }
        }
    }

    private func sync<T>(_ body: () throws -> T) rethrows -> T { try queue.sync(execute: body) }

    // MARK: 电脑

    public func computers() throws -> [Computer] {
        try sync {
            try db.query("select id, name, platform, server, device_id, desktop_key from computer order by added_at, id") { r in
                Computer(
                    id: r.text(0), name: r.text(1), platform: r.text(2), server: URL(string: r.text(3)) ?? URL(filePath: "/"),
                    deviceId: r.text(4), desktopKey: Data(base64Encoded: r.text(5)) ?? Data())
            }
        }
    }

    /// 加一台电脑。重新配对同一台时换成新的，以前的任务缓存留着，没送出的命令作废（旧设备已经失效）。
    public func add(_ c: Computer, now: Int64) throws {
        try sync {
            try db.transaction {
                try db.run("delete from outbox where computer = ?", .text(c.id))
                try db.run(
                    """
                    insert into computer (id, name, platform, server, device_id, desktop_key, added_at) values (?, ?, ?, ?, ?, ?, ?)
                    on conflict (id) do update set name = excluded.name, platform = excluded.platform, server = excluded.server,
                      device_id = excluded.device_id, desktop_key = excluded.desktop_key
                    """,
                    .text(c.id), .text(c.name), .text(c.platform), .text(c.server.absoluteString), .text(c.deviceId),
                    .text(c.desktopKey.base64EncodedString()), .int(now))
            }
        }
    }

    public func rename(_ computer: String, to name: String) throws {
        try sync { try db.run("update computer set name = ? where id = ?", .text(name), .text(computer)) }
    }

    public func remove(_ computer: String) throws {
        try sync { try db.run("delete from computer where id = ?", .text(computer)) }
    }

    // MARK: 总览

    public func overview(_ computer: String) throws -> Overview? {
        try sync {
            try db.query("select json from overview where computer = ?", .text(computer)) { $0.text(0) }.first
                .flatMap { try? JSONDecoder().decode(Overview.self, from: Data($0.utf8)) }
        }
    }

    /// 换成新的总览；比现有的旧就不动，返回 false。已经没有的任务，它的事件一起清掉。
    public func save(_ overview: Overview, for computer: String) throws -> Bool {
        let json = try Self.encode(overview)
        return try sync {
            try db.transaction {
                let current = try db.query("select rev from overview where computer = ?", .text(computer)) { $0.int(0) }.first
                if let current, current >= overview.rev { return false }
                try db.run("insert or replace into overview (computer, rev, json) values (?, ?, ?)", .text(computer), .int(overview.rev), .text(json))
                let keep = overview.tasks.map { String($0.id) }.joined(separator: ",")
                for table in ["event", "review"] {
                    try db.run("delete from \(table) where computer = ? and task not in (\(keep))", .text(computer))
                }
                return true
            }
        }
    }

    // MARK: 事件

    public enum Append: Equatable, Sendable {
        /// 接上了；`more` 为真时电脑上还有后面的。
        case applied(more: Bool)
        /// 中间缺了，要从 `after` 之后重新要。
        case gap(after: Int64)
    }

    public func lastSeq(_ computer: String, _ task: Int64) throws -> Int64 {
        try sync { try lastSeqUnlocked(computer, task) }
    }

    private func lastSeqUnlocked(_ computer: String, _ task: Int64) throws -> Int64 {
        try db.query("select coalesce(max(seq), 0) from event where computer = ? and task = ?", .text(computer), .int(task)) { $0.int(0) }.first ?? 0
    }

    /// 存事件。只接连续的；电脑上最后的编号比手机上的还小，说明那边重来过，手机上的作废。
    public func append(_ events: [TaskEvent], task: Int64, latest: Int64, for computer: String) throws -> Append {
        let rows = try events.sorted { $0.seq < $1.seq }.map { ($0.seq, try Self.encode($0)) }
        return try sync {
            try db.transaction {
                var last = try lastSeqUnlocked(computer, task)
                if latest < last {
                    try db.run("delete from event where computer = ? and task = ?", .text(computer), .int(task))
                    last = 0
                    if rows.first?.0 != 1 { return .gap(after: 0) }
                }
                if last > 0, let first = rows.first(where: { $0.0 > last })?.0, first != last + 1 { return .gap(after: last) }
                for (seq, json) in rows where seq > last {
                    if last > 0, seq != last + 1 { return .gap(after: last) }
                    try db.run(
                        "insert or ignore into event (computer, task, seq, json) values (?, ?, ?, ?)", .text(computer), .int(task), .int(seq), .text(json))
                    last = seq
                }
                return .applied(more: last < latest)
            }
        }
    }

    public func events(_ computer: String, _ task: Int64) throws -> [TaskEvent] {
        try sync {
            try db.query("select json from event where computer = ? and task = ? order by seq", .text(computer), .int(task)) { $0.text(0) }
                .compactMap { try? JSONDecoder().decode(TaskEvent.self, from: Data($0.utf8)) }
        }
    }

    // MARK: 审阅全文

    public func saveReview(_ output: String, finishedAt: Int64, task: Int64, for computer: String) throws {
        try sync {
            try db.run(
                "insert or replace into review (computer, task, finished_at, output) values (?, ?, ?, ?)", .text(computer), .int(task),
                .int(finishedAt), .text(output))
        }
    }

    public func review(_ computer: String, _ task: Int64) throws -> (finishedAt: Int64, output: String)? {
        try sync {
            try db.query("select finished_at, output from review where computer = ? and task = ?", .text(computer), .int(task)) {
                ($0.int(0), $0.text(1))
            }.first
        }
    }

    // MARK: 待发送

    public func enqueue(_ item: OutboxItem, for computer: String) throws {
        let json = try Self.encode(item.command)
        try sync {
            try db.run(
                "insert or replace into outbox (id, computer, json, at, state, error) values (?, ?, ?, ?, ?, ?)", .text(item.id),
                .text(computer), .text(json), .int(item.at), .text(item.state.rawValue), item.error.map { .text($0) } ?? .null)
        }
    }

    public func outbox(_ computer: String) throws -> [OutboxItem] {
        try sync {
            try db.query("select id, json, at, state, error from outbox where computer = ? order by at, id", .text(computer)) { r in
                (r.text(0), r.text(1), r.int(2), r.text(3), r.optionalText(4))
            }.compactMap { id, json, at, state, error in
                guard let command = try? JSONDecoder().decode(Command.self, from: Data(json.utf8)), let state = OutboxItem.State(rawValue: state)
                else { return nil }
                return OutboxItem(id: id, command: command, at: at, state: state, error: error)
            }
        }
    }

    public func mark(_ id: String, _ state: OutboxItem.State, error: String? = nil) throws {
        try sync {
            try db.run("update outbox set state = ?, error = ? where id = ?", .text(state.rawValue), error.map { .text($0) } ?? .null, .text(id))
        }
    }

    public func dequeue(_ id: String) throws {
        try sync { try db.run("delete from outbox where id = ?", .text(id)) }
    }

    private static func encode(_ value: some Encodable) throws -> String {
        String(decoding: try JSONEncoder().encode(value), as: UTF8.self)
    }
}
