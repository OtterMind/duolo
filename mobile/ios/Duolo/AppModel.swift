import DuoloKit
import Network
import Observation
import SwiftUI
import UIKit

/// 界面用的全部状态。数据都从 `LocalStore` 读，`SyncEngine` 说变了就重新读一遍，界面不直接碰网络。
@MainActor @Observable
final class AppModel {
    // MARK: 数据

    private(set) var computers: [Computer] = []
    private(set) var current: Computer?
    private(set) var overview: Overview?
    private(set) var rows: [TaskRow] = []
    private(set) var link: SyncEngine.Link = .connecting
    private(set) var outbox: [OutboxItem] = []
    private(set) var opened: OpenedTask?
    /// 要告诉用户的错误，显示几秒。
    var problem: AppError?
    /// 等用户确认的配对链接。
    var pendingPairing: PairingLink?

    struct OpenedTask {
        let id: Int64
        var timeline: Timeline
        var review: String?
    }

    // MARK: 设置

    /// 界面语言：`auto` 跟随系统，和桌面端一样。
    var language: String { didSet { defaults.set(language, forKey: "language"); t = Self.localizer(language) } }
    /// `auto`、`light`、`dark`。
    var appearance: String { didSet { defaults.set(appearance, forKey: "appearance") } }
    private(set) var t: Localizer
    private(set) var project: String? { didSet { defaults.set(project, forKey: "project.\(current?.id ?? "")") } }

    /// 正在进行的语音输入，面板由最上层的界面画。
    let voice = VoiceSession()

    private let defaults = UserDefaults.standard
    private let store: LocalStore
    private let secrets: any SecretStore = Keychain()
    private var engine: SyncEngine?
    private var listener: Task<Void, Never>?
    private let network = NWPathMonitor()

    init() {
        store = Self.openStore()
        language = UserDefaults.standard.string(forKey: "language") ?? "auto"
        appearance = UserDefaults.standard.string(forKey: "appearance") ?? "auto"
        t = Self.localizer(UserDefaults.standard.string(forKey: "language") ?? "auto")
        reloadComputers()
        select(computers.first { $0.id == defaults.string(forKey: "computer") } ?? computers.first)
        Self.watch(network, self)
    }

    /// 网络恢复时马上重连。回调在后台线程，所以在不带主线程隔离的地方建（理由同 `VoiceInput.allowed`）。
    private nonisolated static func watch(_ monitor: NWPathMonitor, _ model: AppModel) {
        monitor.pathUpdateHandler = { [weak model] path in
            guard path.status == .satisfied else { return }
            Task { @MainActor in await model?.engine?.nudge() }
        }
        monitor.start(queue: .global(qos: .utility))
    }

    /// 打开数据库。坏了就挪到一边重新建：里面只是缓存，配对信息丢了重新扫码即可，不能让应用打不开。
    private static func openStore() -> LocalStore {
        let fm = FileManager.default
        let dir = URL.applicationSupportDirectory
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appending(path: "duolo.sqlite")
        if let store = try? LocalStore(path: file.path) {
            try? fm.setAttributes([.protectionKey: FileProtectionType.completeUntilFirstUserAuthentication], ofItemAtPath: file.path)
            return store
        }
        let damaged = "duolo.sqlite.damaged-\(Int(Date().timeIntervalSince1970))"
        for suffix in ["", "-wal", "-shm"] {
            try? fm.moveItem(at: dir.appending(path: "duolo.sqlite\(suffix)"), to: dir.appending(path: damaged + suffix))
        }
        // 内存数据库一定能打开：手机满了也能看到界面和错误。
        return (try? LocalStore(path: file.path)) ?? (try! LocalStore(path: nil))
    }

    private static func localizer(_ language: String) -> Localizer {
        let code = language == "auto" ? Localizer.match(Locale.preferredLanguages) : Localizer.match([language])
        return Localizer(code: code, directory: Bundle.main.bundleURL.appending(path: "locales"))
    }

    var colorScheme: ColorScheme? {
        switch appearance {
        case "light": .light
        case "dark": .dark
        default: nil
        }
    }

    // MARK: 电脑

    private func reloadComputers() {
        computers = (try? store.computers()) ?? []
        if let current { self.current = computers.first { $0.id == current.id } ?? current }
    }

    func select(_ computer: Computer?) {
        guard computer?.id != current?.id || engine == nil else { return }
        listener?.cancel()
        if let engine { Task { await engine.stop() } }
        engine = nil
        current = computer
        opened = nil
        link = .connecting
        defaults.set(computer?.id, forKey: "computer")
        project = computer.flatMap { defaults.string(forKey: "project.\($0.id)") }
        reloadOverview()
        reloadOutbox()
        guard let computer else { return }
        guard let secret = try? secrets.load(computer.id),
            let engine = try? SyncEngine(computer: computer, secrets: secret, store: store)
        else {
            // 钥匙串里没有（例如从备份恢复到新手机）：要重新配对。
            link = .unlinked
            return
        }
        self.engine = engine
        listener = Task { [weak self] in
            let changes = await engine.changes()
            await engine.start()
            for await change in changes { self?.apply(change) }
        }
    }

    private func apply(_ change: SyncEngine.Change) {
        switch change {
        case .link(let next):
            link = next
            reloadComputers()
        case .overview: reloadOverview()
        case .task(let id): if opened?.id == id { reloadTask() }
        case .outbox: reloadOutbox()
        case .failure(let error): problem = error
        }
    }

    func pair(_ link: PairingLink) async throws {
        let computer = try await Pairing(store: store, secrets: secrets)
            .connect(link, deviceName: UIDevice.current.name, platform: "ios", now: Int64(Date().timeIntervalSince1970 * 1000))
        reloadComputers()
        // 重新配对同一台时也要换上新的密钥。
        if let engine { Task { await engine.stop() } }
        engine = nil
        current = nil
        select(computer)
    }

    func disconnect(_ computer: Computer) async {
        if computer.id == current?.id { await engine?.stop() }
        do {
            try await Pairing(store: store, secrets: secrets).disconnect(computer)
        } catch {
            problem = .storage
        }
        computers = (try? store.computers()) ?? []
        if computer.id == current?.id {
            engine = nil
            current = nil
            select(computers.first)
        }
    }

    // MARK: 前后台

    func becameActive() {
        Task {
            await engine?.start()
            await engine?.nudge()
        }
    }

    func wentToBackground() {
        // 留一点时间把刚发的送出去，再断开。
        let app = UIApplication.shared
        let id = app.beginBackgroundTask()
        Task { [engine] in
            try? await Task.sleep(for: .seconds(5))
            if app.applicationState == .background { await engine?.stop() }
            app.endBackgroundTask(id)
        }
    }

    // MARK: 任务

    private func reloadOverview() {
        overview = current.flatMap { try? store.overview($0.id) }
        rows = TaskRow.rows(overview?.tasks ?? [])
        // 没选过（或者选的没了）就定在现在这个，不能看着看着自己跳到别的仓库。
        if let shown = currentProject?.id, shown != project { project = shown }
        if opened != nil { reloadTask() }
    }

    private func reloadOutbox() {
        outbox = current.flatMap { try? store.outbox($0.id) } ?? []
    }

    /// 正在看的仓库：选过的，不然是最近有动静的任务所在的。
    var currentProject: ProjectOverview? {
        let projects = overview?.projects ?? []
        if let chosen = projects.first(where: { $0.id == project }) { return chosen }
        let recent = overview?.tasks.max { $0.updatedAt < $1.updatedAt }?.project
        return projects.first { $0.id == recent } ?? projects.first
    }

    func choose(project: String) { self.project = project }

    func rows(in project: String?) -> [TaskRow] { rows.filter { $0.task.project == project } }

    /// 别的仓库里有任务在等。
    var othersNeedYou: Bool { rows.contains { $0.group == .attention && $0.task.project != currentProject?.id } }

    func row(_ id: Int64) -> TaskRow? { rows.first { $0.id == id } }

    func title(_ task: TaskOverview) -> String { task.untitled ? t("task.untitled") : task.title }

    func open(_ id: Int64) {
        opened = OpenedTask(id: id, timeline: Timeline())
        reloadTask()
        Task { await engine?.open(id) }
    }

    func leave(_ id: Int64) {
        guard opened?.id == id else { return }
        opened = nil
        Task { await engine?.open(nil) }
    }

    private func reloadTask() {
        guard let current, let id = opened?.id else { return }
        let events = (try? store.events(current.id, id)) ?? []
        opened = OpenedTask(id: id, timeline: Timeline(events), review: (try? store.review(current.id, id))?.output)
    }

    func refresh() async { await engine?.refresh() }

    /// 让电脑做一件事，出错时告诉用户。返回有没有成功交出去。
    @discardableResult
    func submit(_ command: Command) async -> Bool {
        guard let engine else {
            problem = .unauthorized
            return false
        }
        do {
            try await engine.submit(command)
            return true
        } catch {
            problem = error as? AppError ?? .server
            return false
        }
    }

    func retry(_ item: OutboxItem) {
        Task {
            do { try await engine?.retry(item.id) } catch { problem = error as? AppError ?? .storage }
        }
    }

    func discard(_ item: OutboxItem) { Task { try? await engine?.discard(item.id) } }

    /// 给这个任务的、还没送到或没成功的话。
    func pending(for task: Int64) -> [OutboxItem] { outbox.filter { $0.command.task == task } }

    var computerOnline: Bool { link == .connected(computerOnline: true) }

    // MARK: 语音

    /// 语音识别用当前电脑配对的服务器和登录凭据；没配对时为空。
    func voiceRoute() -> (server: URL, token: String)? {
        guard let current, let secret = try? secrets.load(current.id) else { return nil }
        return (current.server, secret.token)
    }

    /// 给识别服务的提示：打开着的任务的文件名、代码名字和最近的对话；没打开任务时只有项目名。
    func voiceHints() -> VoiceHints {
        let projectName = overview?.projects.first { $0.id == project }?.name
        guard let current, let id = opened?.id, let task = rows.first(where: { $0.id == id })?.task else {
            return VoiceHints(project: projectName, title: nil, branch: nil, events: [])
        }
        let events = (try? store.events(current.id, id)) ?? []
        let name = overview?.projects.first { $0.id == task.project }?.name ?? projectName
        return VoiceHints(project: name, title: task.untitled ? nil : task.title, branch: task.branch, events: events)
    }

    /// 错误给用户看的说明。
    func message(_ problem: AppError) -> String {
        switch problem.code {
        case "voicePermission": t("mobile.voice.permission")
        default: t.error(problem.code)
        }
    }

    // MARK: 配对链接

    func handle(url: URL) {
        if let link = PairingLink(url.absoluteString) { pendingPairing = link } else { problem = .badPairingLink }
    }

    // MARK: 时间

    func ago(_ ms: Int64) -> String {
        let f = RelativeDateTimeFormatter()
        f.locale = t.locale
        f.unitsStyle = .short
        let date = Date(timeIntervalSince1970: Double(ms) / 1000)
        return f.localizedString(for: min(date, .now), relativeTo: .now)
    }

    func lasting(since ms: Int64) -> String {
        let f = DateComponentsFormatter()
        var calendar = Calendar.current
        calendar.locale = t.locale
        f.calendar = calendar
        f.unitsStyle = .abbreviated
        f.maximumUnitCount = 1
        f.allowedUnits = [.day, .hour, .minute]
        return f.string(from: max(60, Date.now.timeIntervalSince1970 - Double(ms) / 1000)) ?? ""
    }
}
