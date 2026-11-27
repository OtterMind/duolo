import Foundation

/// 和一台电脑的同步。一台电脑一个，全部状态只在这个 actor 里改。
///
/// 稳定性靠这几条（详见 docs/mobile.md）：
/// - 收到的消息先存进数据库再确认，没存成就不确认，服务器下次补发；
/// - 重复、乱序、缺了中间的都能处理：总览看版本号，事件看编号，缺了就重新要；
/// - 命令先存再发，同一个编号重发不会执行两次；回答和打断只在电脑在线时发，不排队；
/// - 断线按退避重连，半死的连接靠心跳发现。
public actor SyncEngine {
    public enum Link: Equatable, Sendable {
        case connecting
        /// 连上服务器了；电脑不一定在线。
        case connected(computerOnline: Bool)
        /// 断了，等一会儿重连。
        case waiting
        /// 这台手机被解除了，要重新配对。
        case unlinked
    }

    public enum Change: Equatable, Sendable {
        case link(Link)
        case overview
        case task(Int64)
        case outbox
        case failure(AppError)
    }

    public struct Timing: Sendable {
        public var ping: Duration = .seconds(25)
        public var silence: Duration = .seconds(60)
        public var reply: Duration = .seconds(15)
        public var backoff: @Sendable (Int) -> Duration = { Backoff.jittered($0) }

        public init() {}
    }

    public nonisolated let computer: Computer
    private let store: LocalStore
    private let token: String
    private let channel: SecureChannel
    private let transport: any RelayTransport
    private let timing: Timing
    private let now: @Sendable () -> Int64

    public private(set) var link: Link = .connecting
    private var connection: (any RelayConnection)?
    private var lastHeard = ContinuousClock.now
    private var attempt = 0
    private var runner: Task<Void, Never>?
    private var sleeper: Task<Void, Never>?
    /// 打开着的任务，电脑上线后接着补它的事件。
    private var focus: Int64?
    /// 第一次打开、还没有事件的任务：要的是从哪条之后（见 `fetch`）。回来之前电脑推来的新事件不存，
    /// 否则手机以为已经有到最新的了，随后回来的前面的事件会被当成旧的丢掉。
    private var opening: [Int64: Int64] = [:]
    /// 在等结果的实时命令（回答、打断）。
    private var live: [String: CheckedContinuation<Void, any Error>] = [:]
    private var observers: [UUID: AsyncStream<Change>.Continuation] = [:]

    public init(
        computer: Computer, secrets: Secrets, store: LocalStore, transport: any RelayTransport = URLSessionTransport(), timing: Timing = Timing(),
        now: @escaping @Sendable () -> Int64 = { Int64(Date().timeIntervalSince1970 * 1000) }
    ) throws {
        self.computer = computer
        self.store = store
        self.transport = transport
        self.timing = timing
        self.now = now
        token = secrets.token
        channel = try SecureChannel(privateKey: secrets.privateKey, desktopKey: computer.desktopKey, secret: secrets.secret)
    }

    // MARK: 生命周期

    public func start() {
        guard runner == nil, link != .unlinked else { return }
        runner = Task { [weak self] in await self?.run() }
    }

    /// 应用进后台、换了电脑：断开，不再重连。
    public func stop() {
        runner?.cancel()
        runner = nil
        sleeper?.cancel()
        connection?.close()
        connection = nil
        failLive(.network)
    }

    /// 网络恢复、回到前台：不等退避，马上重连。
    public func nudge() {
        attempt = 0
        sleeper?.cancel()
    }

    public func changes() -> AsyncStream<Change> {
        AsyncStream { continuation in
            let id = UUID()
            observers[id] = continuation
            continuation.yield(.link(link))
            continuation.onTermination = { [weak self] _ in Task { await self?.drop(id) } }
        }
    }

    private func drop(_ id: UUID) { observers[id] = nil }

    private func emit(_ change: Change) {
        for observer in observers.values { observer.yield(change) }
    }

    private func set(_ next: Link) {
        guard link != next else { return }
        link = next
        emit(.link(next))
    }

    private func run() async {
        while !Task.isCancelled {
            set(.connecting)
            let closed = await connectOnce()
            connection?.close()
            connection = nil
            failLive(.network)
            if Task.isCancelled { return }
            if closed == 4401 {
                set(.unlinked)
                runner = nil
                return
            }
            set(.waiting)
            let delay = timing.backoff(attempt)
            attempt += 1
            let sleep = Task { _ = try? await Task.sleep(for: delay) }
            sleeper = sleep
            await withTaskCancellationHandler { await sleep.value } onCancel: { sleep.cancel() }
        }
    }

    /// 连一次，直到断开。返回关闭码。
    private func connectOnce() async -> Int {
        do {
            let connection = try await transport.open(server: computer.server, token: token)
            self.connection = connection
            lastHeard = .now
            let heartbeat = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(for: self?.timing.ping ?? .seconds(25))
                    if Task.isCancelled { return }
                    await self?.beat()
                }
            }
            defer { heartbeat.cancel() }
            while !Task.isCancelled {
                let text = try await connection.receive()
                lastHeard = .now
                await handle(text)
            }
            return 0
        } catch let closed as RelayClosed {
            return closed.code
        } catch {
            return 0
        }
    }

    /// 心跳：太久没有任何消息，说明连接已经断了但系统还没发现。
    private func beat() async {
        if ContinuousClock.now - lastHeard > timing.silence {
            connection?.close()
        } else {
            await transmit(.ping)
        }
    }

    // MARK: 收

    private func handle(_ text: String) async {
        guard let message = try? JSONDecoder().decode(RelayIncoming.self, from: Data(text.utf8)) else { return }
        switch message {
        case .hello(let peers):
            attempt = 0
            let peer = peers.first { $0.id == computer.id }
            if let name = peer?.name, name != computer.name { try? store.rename(computer.id, to: name) }
            set(.connected(computerOnline: peer?.online ?? false))
            await flushOutbox()
            if peer?.online == true { await catchUp() }
        case .presence(let device, let online):
            guard device == computer.id else { return }
            set(.connected(computerOnline: online))
            if online { await catchUp() }
        case .message(let id, let from, let body):
            // 同一账号的其他设备发来的不归这里管，但要确认掉，不然会一直补发。
            guard from == computer.id, let plaintext = try? channel.open(body, id: id, from: from, to: computer.deviceId) else {
                await transmit(.ack([id]))
                return
            }
            do {
                try await apply(plaintext)
                await transmit(.ack([id]))
            } catch {
                // 没存成（例如手机满了）：不确认，以后补发。
                emit(.failure(.storage))
            }
        case .sent(let id):
            if (try? store.outbox(computer.id))?.contains(where: { $0.id == id && $0.state == .queued }) == true {
                try? store.mark(id, .sent)
                emit(.outbox)
            }
        case .error(let code, let id):
            guard let id else { return }
            let error = AppError(code == "unknownDevice" ? "unauthorized" : "server")
            finishLive(id, .failure(error))
            try? store.mark(id, .failed, error: error.code)
            emit(.outbox)
        case .other:
            break
        }
    }

    private func apply(_ plaintext: Data) async throws {
        // 读不懂的（更新的格式）当作没有，不能卡住后面的消息。
        guard let message = try? JSONDecoder().decode(DesktopMessage.self, from: plaintext) else { return }
        switch message {
        case .overview(let overview):
            if try store.save(overview, for: computer.id) { emit(.overview) }
        case .events(let task, let events, let latest):
            // 没打开过的任务不存事件，列表用总览就够了。
            let known = try store.lastSeq(computer.id, task)
            guard task == focus || known > 0 else { return }
            if known == 0 {
                // 还没有这个任务的事件：只认刚才要的那一段（从 `after` 的下一条开始），推来的新事件等它回来再补。
                guard let after = opening[task], events.min(by: { $0.seq < $1.seq })?.seq == after + 1 else { return }
                opening[task] = nil
            }
            switch try store.append(events, task: task, latest: latest, for: computer.id) {
            case .applied(let more):
                emit(.task(task))
                if more, !events.isEmpty { await fetch(task) }
            case .gap(let after):
                emit(.task(task))
                await request(.fetch(task: task, after: after))
            }
        case .review(let task, let finishedAt, let output):
            try store.saveReview(output, finishedAt: finishedAt, task: task, for: computer.id)
            emit(.task(task))
        case .result(let id, let result):
            if result.ok {
                try store.dequeue(id)
                finishLive(id, .success(()))
            } else {
                let error = AppError(result.error ?? "internal")
                try store.mark(id, .failed, error: error.code)
                finishLive(id, .failure(error))
            }
            emit(.outbox)
        case .unknown:
            break
        }
    }

    // MARK: 要

    /// 电脑在线了：要最新的总览，接着补打开着的任务。
    private func catchUp() async {
        await request(.sync)
        if let focus { await fetch(focus) }
    }

    private func fetch(_ task: Int64) async {
        guard let last = try? store.lastSeq(computer.id, task) else { return }
        var after = last
        if last == 0, let overview = try? store.overview(computer.id), let seq = overview.tasks.first(where: { $0.id == task })?.seq {
            // 第一次打开：只要最近的一段，很长的任务不从头拉。
            after = max(0, seq - 400)
        }
        if last == 0 { opening[task] = after }
        await request(.fetch(task: task, after: after))
    }

    /// 打开、离开任务。
    public func open(_ task: Int64?) async {
        focus = task
        if let task, link == .connected(computerOnline: true) { await fetch(task) }
    }

    /// 下拉刷新。
    public func refresh() async {
        if link == .connected(computerOnline: true) { await catchUp() } else { nudge() }
    }

    // MARK: 发

    /// 让电脑做一件事。回答和打断等电脑的结果（不成抛错误）；其他的先存下来，电脑离线也会送到。
    public func submit(_ command: Command) async throws {
        let item = OutboxItem(id: UUID().uuidString.lowercased(), command: command, at: now(), state: .queued)
        if command.isLive {
            guard link == .connected(computerOnline: true) else { throw AppError.computerOffline }
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                live[item.id] = continuation
                Task {
                    if await !transmit(item) { finishLive(item.id, .failure(AppError.network)) }
                }
                Task {
                    try? await Task.sleep(for: timing.reply)
                    finishLive(item.id, .failure(AppError.network))
                }
            }
            return
        }
        do { try store.enqueue(item, for: computer.id) } catch { throw AppError.storage }
        emit(.outbox)
        if case .connected = link { await transmit(item) }
    }

    /// 没送到的再发一次（用新编号：旧编号电脑可能已经处理过并拒绝了）。
    public func retry(_ id: String) async throws {
        guard let item = try store.outbox(computer.id).first(where: { $0.id == id && $0.state == .failed }) else { return }
        try store.dequeue(id)
        try await submit(item.command)
    }

    public func discard(_ id: String) throws {
        try store.dequeue(id)
        emit(.outbox)
    }

    /// 重新连上：服务器还没收下的全部再发一遍，编号不变，重复的服务器会去掉。
    private func flushOutbox() async {
        for item in (try? store.outbox(computer.id)) ?? [] where item.state == .queued {
            await transmit(item)
        }
    }

    @discardableResult
    private func transmit(_ item: OutboxItem) async -> Bool {
        await send(.command(id: item.id, at: item.at, command: item.command), id: item.id)
    }

    private func request(_ message: PhoneMessage) async {
        await send(message, id: UUID().uuidString.lowercased())
    }

    @discardableResult
    private func send(_ message: PhoneMessage, id: String) async -> Bool {
        guard let plaintext = try? JSONEncoder().encode(message),
            let body = try? channel.seal(plaintext, id: id, from: computer.deviceId, to: computer.id)
        else { return false }
        return await transmit(.send(to: computer.id, id: id, body: body))
    }

    @discardableResult
    private func transmit(_ message: RelayOutgoing) async -> Bool {
        guard let connection, let data = try? JSONEncoder().encode(message) else { return false }
        do {
            try await connection.send(String(decoding: data, as: UTF8.self))
            return true
        } catch {
            // 发不出去就是断了，接收那边会发现并重连。
            connection.close()
            return false
        }
    }

    private func finishLive(_ id: String, _ result: Result<Void, AppError>) {
        guard let continuation = live.removeValue(forKey: id) else { return }
        switch result {
        case .success: continuation.resume()
        case .failure(let error): continuation.resume(throwing: error)
        }
    }

    private func failLive(_ error: AppError) {
        for id in Array(live.keys) { finishLive(id, .failure(error)) }
    }
}
