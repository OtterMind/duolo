# 手机端

iOS（Swift / SwiftUI）和 Android（Kotlin / Compose）各用原生语言写，**行为由同一份契约约束**，文案和桌面端共用 `locales/*.json`。

```
mobile/
  contract/          三端共用的样例：wire.json（消息格式）、crypto.json（加密）、behavior.json（列表、时间线、配对链接、语言匹配）
  ios/DuoloKit/      核心（Swift 包，不依赖界面和第三方库）
  ios/Duolo/         界面（SwiftUI，iOS 17+）；工程由 XcodeGen 从 project.yml 生成
  android/core/      核心（纯 Kotlin，不依赖 Android）
  android/app/       界面（Compose，Android 8+）
  fonts/  scripts/   共用字体；生成图标、应用图标、iOS 授权说明的脚本
server/dev/mobile-sandbox.ts   联调沙箱：真的服务端 + 模拟电脑
```

## 现状

- 两个手机端都能配对、收发、回答、新建、审阅、断开，用沙箱端到端测过。
- **桌面应用还没有接服务端**：登录、显示二维码、按下面的约定收发还没做。接上之前手机连不了真的电脑。
- 还没有推送通知（服务端也还不会发）；语音的"整理成指令"需要电脑端的整理命令，手机上没有。

## 分层

依赖只能自上而下，界面不碰网络和数据库：

| 层 | 职责 | iOS | Android |
|---|---|---|---|
| 界面 | 显示、手势、系统能力（相机、语音、钥匙串） | `Duolo/` | `app/` |
| 界面状态 | 从本地数据推导界面要的东西 | `AppModel` | `AppModel` |
| 同步 | 连接、收发、重试、补齐 | `SyncEngine`（actor） | `SyncEngine`（单线程调度器） |
| 本地数据 | SQLite，唯一的事实来源 | `LocalStore` | `LocalStore` |
| 推导 | 列表行、时间线、文案 | `TaskRow`、`Timeline`、`Localizer` | 同名 |
| 协议 | 消息、加密、配对链接 | `Wire/` | `Wire.kt` 等 |

数据单向流动：服务器 → `SyncEngine` → `LocalStore` → `AppModel` → 界面。界面的操作反过来只经过 `SyncEngine.submit`。

## 消息

明文格式在 `crates/duolo-core/src/wire.rs`，每条带 `v: 1`。字段只增不改；**不认识的 `type` 忽略，不认识的事件原样存下来**，新旧版本可以互通。

手机 → 电脑：

| `type` | 用途 |
|---|---|
| `sync` | 要一份最新总览 |
| `fetch { task, after }` | 要任务第 `after` 条之后的事件 |
| `command { id, at, command }` | 执行命令（`event.rs` 的 `Command`） |

电脑 → 手机：

| `type` | 用途 |
|---|---|
| `overview { rev, projects, tasks, … }` | 列表页的全部内容，整份替换；`rev` 比手机上的旧就忽略。每个任务带着电脑算好的 `state`（见下一节） |
| `events { task, events, latest }` | 任务的事件，编号连续；`latest` 是电脑上最后一条 |
| `review { task, finishedAt, output }` | 最近一次审阅的全文，`fetch` 时一起发 |
| `result { id, result }` | 命令的结果 |

电脑端要遵守的：

1. **只给在线的手机推送**。离线的手机上线后自己会要，不往离线信箱里堆。
2. **同一个命令编号只执行一次**，再来就回同样的结果。执行过的编号至少记 8 天（信箱存 7 天）。
3. **太晚的不执行**：`Command::is_expired`，回 `commandExpired`。
4. **回答要核对提示**：`Answer.prompt` 不是现在这条、或者没有这个选项，回 `promptChanged`（`Engine::answer` 已经这样做）。手机发来的 `Answer` 没带 `prompt` 一律拒绝。
5. 一条消息加密后不超过服务器的上限（默认 256 KB）：`events` 分页，一次最多 200 条，超长的正文截断。
6. 收到解不开的消息：确认掉，不处理。

## 和电脑端保持一致

- **任务状态只算一次**：`duolo-core/src/wire.rs` 的 `TaskState::of` 把写代码的 Agent 的状态和审阅结论合成一个状态，
  桌面快照（`view.rs` 的 `TaskView.state`）和发给手机的总览都带着它，电脑侧边栏和两个手机端都不自己推导。
  规则的样例在 `contract/behavior.json` 的 `rows`，由 Rust 的测试核对；手机端的测试只核对分组和第二行。
- **颜色的意思相同**：琥珀色只表示等你确认；红色是要你处理的问题（审出问题、审阅失败、异常退出）；
  绿色只表示审阅通过；蓝色在写，紫色在审；灰色是没有下一步（写完了、审完了、已结束）。
- **状态图标相同**：纯色圆底加白色 Octicons 图标，放在行首；底色两个主题一样（`StateFill` / `.state-badge`），
  进行中的圆小一圈、外面一段弧线在转，审出问题时圆里写问题数，未启动是虚线圈。列表上不写状态名（读屏和悬停提示里有），
  第二行是最新的一句话；只有等你确认在第二行前面写出状态名。
- **文案相同**：状态名都是 `state.<code>`，"N 个任务在等你确认"都是 `sidebar.waiting`，都叫"项目"。
- **项目**：两边的项目和顺序一样（`state.json` 里的顺序）。电脑把所有项目排开、项目内按创建先后，行不会跳；
  手机一次看一个项目、按状态排（要处理 → 进行中 → 完成）。电脑顶部和手机列表顶部都有跨项目的"在等你确认"入口。
  任务改的是项目里的某个仓库时，两边都显示仓库标签（`repo`）。
- **Agent 图标**：来自插件自带的 `icon.svg`，颜色是插件的 `icon_color`（随数据发给手机）。插件有增减时运行
  `node mobile/scripts/gen-agent-icons.mjs`；没有图标的 Agent 两边都显示首字母。代码里不写死任何 Agent。

## 端到端加密

服务器只转发密文，它换掉公钥也看不到内容、冒充不了任何一边。

1. 电脑显示二维码：`duolo://pair?v=1&server=…&code=…&key=…&secret=…`
   - `code`：服务器发的一次性配对码（10 分钟）；
   - `key`：电脑的 X25519 公钥；`secret`：32 字节随机数，**只在二维码里，不经过服务器**（都是 base64url）。
2. 手机新生成一对 X25519 密钥，用 `code` 登记公钥。服务器返回的电脑公钥和二维码里的不一样就中止。
3. 两边各算：
   `key(方向) = HKDF-SHA256(ikm = X25519 共享密钥, salt = secret, info = "duolo/v1 " + 方向 + 电脑公钥 + 手机公钥, 32 字节)`，
   方向是 `phone->desktop` 或 `desktop->phone`。共享密钥全 0 时拒绝。
4. 每条消息用 ChaCha20-Poly1305，随机 12 字节 nonce，
   附加数据是 `duolo/v1\n<消息编号>\n<发送设备>\n<接收设备>`，所以服务器不能改编号或换收件人重放。
   `body = base64(0x01 + nonce + 密文 + 16 字节校验)`。
5. 电脑第一次收到某台手机的消息时，用还没用掉的 `secret` 逐个试，能解开的就绑定给它，之后不再接受别的手机用这个 `secret`。

没有前向保密：密钥从配对起不变，重新配对才换。令牌、私钥和 `secret` 在钥匙串 / 系统密钥库里，不进备份。

链接里的服务器只接受 `https`；`http` 只许本机和局域网地址。点开链接先问用户，不直接连。

## 稳定性

- **先存后确认**：收到的消息写进数据库才 `ack`。没存成（手机满了）就不确认，服务器下次补发。
- **重复、乱序、丢失都能恢复**：总览看 `rev`；事件只接连续的，缺了就 `fetch`；电脑上的编号比手机上的还小，说明那边重来过，手机上的作废重取。
- **命令先存再发**：新建、发消息、审阅、关闭存进发件箱，断线、杀进程都不丢，重连后用同一个编号再发；被拒绝的留在界面上，可以重试或放弃。
- **回答和打断不排队**：电脑不在线就直接告诉用户，晚到的"允许"可能批准另一件事。
- **重连**：1、2、4…30 秒退避加随机抖动，网络恢复和回到前台时马上重试；25 秒心跳，60 秒没有任何消息就当断了。
- **解除**：关闭码 4401 不再重连，提示重新配对。
- **坏数据不崩溃**：解不开的消息、读不懂的格式都跳过；数据库打不开就改名留着重建（里面只是缓存）。
- **数据库升级**只往上加，在事务里做。SQL 只用 SQLite 3.18（Android 8）就有的写法。
- 后台 5 秒后断开连接，回到前台重连并补齐；不做常驻后台。

## 多语言

和桌面端完全一致，规范见 [i18n.md](i18n.md)：

- 语言包只有一份，`locales/*.json` 在编译时原样打包（iOS 是文件夹引用，Android 是 assets），14 种语言，默认跟随系统，设置里可以换。
- 代码里用 `t("key", …)`，不写死给用户看的句子；手机端专用的键是 `mobile.*`，能复用桌面端的就复用。
- `cd app && pnpm build` 会检查 Swift 和 Kotlin 里用到的键、占位符，以及 iOS 授权说明是不是最新的。
- 日期和时长用系统的本地化格式（`RelativeDateTimeFormatter` 等）。
- iOS 的相机、麦克风授权说明由 `node mobile/scripts/gen-ios-strings.mjs` 从 `mobile.perm.*` 生成。

## 改东西的时候

- 改消息格式：先改 `wire.rs` 和 `contract/wire.json`，三端的测试会告诉你哪里没跟上。
- 改任务状态的规则：先改 `contract/behavior.json`，再改 `wire.rs` 的 `TaskState`（电脑和手机都用它）；新状态要在
  `locales/*.json` 加 `state.<code>`，在电脑的 `StatusIcon.tsx` 和两个手机端的 `StatusStyle` 加图标和颜色。
- 改手机的排序、第二行、时间线的规则：先改 `contract/behavior.json`，再改两端。
- 两端的 `SyncEngine` 是同样的结构，改一边要同时改另一边和它的测试。
- 颜色在 `Theme.swift` / `Theme.kt`，深浅成对，两端一致。

## 运行和测试

```sh
# 联调沙箱（先 cd server && docker compose up -d && pnpm install）
pnpm --dir server mobile-sandbox

# iOS
swift test --package-path mobile/ios/DuoloKit
cd mobile/ios && xcodegen && open Duolo.xcodeproj

# Android（模拟器里连沙箱：adb reverse tcp:8787 tcp:8787）
cd mobile/android && ./gradlew :core:test :app:assembleDebug

# 和沙箱真的走一遍
DUOLO_SANDBOX=http://localhost:8788 swift test --package-path mobile/ios/DuoloKit
DUOLO_SANDBOX=http://localhost:8788 mobile/android/gradlew -p mobile/android :core:test
```
