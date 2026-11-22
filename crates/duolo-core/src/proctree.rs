//! 结束一个进程和它启动的所有子进程。
//!
//! Agent 会启动 shell、开发服务器、编译器……只结束 Agent 自己，这些进程会留下来继续占用文件、端口和额度。
//!
//! - macOS / Linux：进程组、会话，加上按父进程找到的后代（Claude Code 给每条命令开新的会话，只看会话找不全）；
//! - Windows：作业对象（job object），子进程自动归到同一个作业里。有窗口的程序（例如 Agent 打开的编辑器、浏览器）不动。

#![allow(unsafe_code)]

use std::process::Child;
use std::time::Duration;

/// 一个进程和它的后代。
#[derive(Debug)]
pub struct ProcessTree(imp::Tree);

impl ProcessTree {
    /// `child` 要由 [`crate::process::spawn_tree`] 启动（自己是一个进程组）。
    pub fn of(child: &Child) -> Self {
        Self(imp::Tree::of(child))
    }

    /// 终端里的进程（自己是一个会话）。
    #[cfg(unix)]
    pub fn of_pid(pid: u32) -> Self {
        Self(imp::Tree::of_pid(pid))
    }

    /// 终端里的进程。
    #[cfg(windows)]
    pub fn of_handle(handle: std::os::windows::io::RawHandle) -> Self {
        Self(imp::Tree::of_handle(handle))
    }

    /// 什么也不管。
    pub fn empty() -> Self {
        Self(imp::Tree::empty())
    }

    /// 现在有哪些进程。先记下来再请主进程退出：主进程退出后，有的后代就看不出是它的了。
    pub fn members(&self) -> Members {
        Members(self.0.members())
    }

    /// 请主进程自己退出（相当于关掉它的终端）。Windows 上没有对应的做法，什么也不做。
    pub fn hang_up(&self) {
        self.0.hang_up();
    }

    /// 强制结束全部进程（包括 `known` 里还在的），最多等 `wait` 让它们真的消失。
    pub fn kill(&self, known: Option<Members>, wait: Duration) {
        self.0.kill(known.map(|m| m.0).unwrap_or_default(), wait);
    }
}

/// [`ProcessTree::members`] 的结果。
#[derive(Debug, Clone)]
pub struct Members(Vec<imp::Member>);

#[cfg(unix)]
mod imp {
    use std::collections::{HashMap, HashSet};
    use std::process::Child;
    #[cfg(not(target_os = "linux"))]
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use libc::pid_t;

    #[derive(Debug)]
    pub struct Tree {
        pid: pid_t,
    }

    /// 进程号和它的会话：隔一会儿再结束时用来确认还是同一个进程。
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Member {
        pid: pid_t,
        sid: pid_t,
    }

    impl Tree {
        pub fn of(child: &Child) -> Self {
            Self::of_pid(child.id())
        }

        pub fn empty() -> Self {
            Self { pid: 0 }
        }

        pub fn of_pid(pid: u32) -> Self {
            Self { pid: pid_t::try_from(pid).unwrap_or(0) }
        }

        pub fn members(&self) -> Vec<Member> {
            let own = std::process::id() as pid_t;
            if self.pid <= 1 || self.pid == own {
                return Vec::new();
            }
            let table = processes();
            let mut wanted: HashSet<pid_t> = HashSet::new();
            // 主进程是我们启动的：父进程是我们，才说明它还在、没被回收，这个进程号还是它的。
            if table.contains(&(self.pid, own)) {
                let mut children: HashMap<pid_t, Vec<pid_t>> = HashMap::new();
                for (pid, parent) in &table {
                    children.entry(*parent).or_default().push(*pid);
                }
                let mut queue = vec![self.pid];
                while let Some(pid) = queue.pop() {
                    if wanted.insert(pid) {
                        queue.extend(children.get(&pid).into_iter().flatten());
                    }
                }
            } else if table.iter().any(|(pid, _)| *pid == self.pid) {
                // 主进程已经退出，这个进程号又分给了别的进程：后面的会话、进程组也不可能是它的了。
                return Vec::new();
            }
            table
                .iter()
                .map(|(pid, _)| *pid)
                .filter(|&pid| pid > 1 && pid != own)
                .filter(|&pid| wanted.contains(&pid) || session(pid) == self.pid || group(pid) == self.pid)
                .map(|pid| Member { pid, sid: session(pid) })
                .collect()
        }

        pub fn hang_up(&self) {
            if self.pid > 1 {
                signal(self.pid, libc::SIGHUP);
            }
        }

        pub fn kill(&self, known: Vec<Member>, wait: Duration) {
            let deadline = Instant::now() + wait;
            let mut targets: HashSet<Member> = known.into_iter().chain(self.members()).collect();
            loop {
                for member in &targets {
                    if alive(*member) {
                        signal(member.pid, libc::SIGKILL);
                    }
                }
                // 结束的同时可能又启动了新的子进程，再找一遍。
                let running: HashSet<pid_t> = processes().into_iter().map(|(pid, _)| pid).collect();
                targets.retain(|m| running.contains(&m.pid) && alive(*m));
                targets.extend(self.members());
                if targets.is_empty() || Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    /// 所有进程的（进程号，父进程号），不含已经退出、等着回收的。
    #[cfg(target_os = "linux")]
    fn processes() -> Vec<(pid_t, pid_t)> {
        let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
        entries
            .flatten()
            .filter_map(|entry| {
                let pid = entry.file_name().to_str()?.parse().ok()?;
                // <进程号> (<名字，可能有空格和括号>) <状态> <父进程号> …
                let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
                let mut words = stat[stat.rfind(')')? + 1..].split_whitespace();
                let state = words.next()?;
                let parent = words.next()?.parse().ok()?;
                (state != "Z" && state != "X").then_some((pid, parent))
            })
            .collect()
    }

    /// 同上（macOS 没有 /proc）。
    #[cfg(not(target_os = "linux"))]
    fn processes() -> Vec<(pid_t, pid_t)> {
        let Ok(out) = Command::new("ps").args(["-axo", "pid=,ppid=,stat="]).stderr(Stdio::null()).output() else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let mut words = line.split_whitespace();
                let (pid, parent) = (words.next()?.parse().ok()?, words.next()?.parse().ok()?);
                (!words.next()?.starts_with('Z')).then_some((pid, parent))
            })
            .collect()
    }

    fn session(pid: pid_t) -> pid_t {
        // SAFETY: 只是查询，不涉及内存访问；进程不存在时返回 -1。
        unsafe { libc::getsid(pid) }
    }

    fn group(pid: pid_t) -> pid_t {
        // SAFETY: 同上。
        unsafe { libc::getpgid(pid) }
    }

    fn signal(pid: pid_t, signal: libc::c_int) {
        // SAFETY: 只发送信号，不涉及内存访问；`pid` 大于 1，不会发给整个系统。
        unsafe { libc::kill(pid, signal) };
    }

    fn alive(member: Member) -> bool {
        session(member.pid) == member.sid && member.sid > 0
    }
}

#[cfg(windows)]
mod imp {
    use std::collections::HashSet;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
    use std::process::Child;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList, QueryInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible};

    /// 一次最多列出多少个进程。
    const MAX_PROCESSES: usize = 4096;

    #[derive(Debug)]
    pub struct Tree {
        /// 建不了作业对象时（极少见）为 None，那就只能结束主进程，由调用方自己处理。
        job: Option<OwnedHandle>,
    }

    pub type Member = u32;

    impl Tree {
        pub fn of(child: &Child) -> Self {
            Self::of_handle(child.as_raw_handle())
        }

        pub fn empty() -> Self {
            Self { job: None }
        }

        pub fn of_handle(process: RawHandle) -> Self {
            // SAFETY: 句柄来自刚启动、还没关闭的子进程；新建的作业句柄交给 OwnedHandle 负责关闭。
            let job = unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                (!job.is_null())
                    .then(|| OwnedHandle::from_raw_handle(job))
                    .filter(|job| AssignProcessToJobObject(job.as_raw_handle(), process) != 0)
            };
            Self { job }
        }

        /// 作业里还在运行的进程。
        pub fn members(&self) -> Vec<Member> {
            let Some(job) = &self.job else { return Vec::new() };
            // JOBOBJECT_BASIC_PROCESS_ID_LIST：两个 u32（总数、列出的个数），后面是 usize 的进程号。
            let mut buf = vec![0usize; MAX_PROCESSES + 1];
            let size = (buf.len() * size_of::<usize>()) as u32;
            // SAFETY: 缓冲区按 usize 对齐，大小如实传入；返回后只读取系统说明已经填好的部分。
            let listed = unsafe {
                let filled = QueryInformationJobObject(
                    job.as_raw_handle(),
                    JobObjectBasicProcessIdList,
                    buf.as_mut_ptr().cast(),
                    size,
                    std::ptr::null_mut(),
                );
                if filled == 0 {
                    return Vec::new();
                }
                *buf.as_ptr().cast::<u32>().add(1) as usize
            };
            // 两个 u32 占 8 字节：64 位上正好一个 usize，32 位上是两个。
            let header = 8 / size_of::<usize>();
            buf[header..].iter().take(listed).map(|&pid| pid as u32).collect()
        }

        pub fn hang_up(&self) {}

        pub fn kill(&self, _known: Vec<Member>, wait: Duration) {
            let deadline = Instant::now() + wait;
            loop {
                let windowed = processes_with_windows();
                let targets: Vec<u32> = self.members().into_iter().filter(|pid| !windowed.contains(pid)).collect();
                for pid in &targets {
                    terminate(*pid);
                }
                if targets.is_empty() || Instant::now() >= deadline {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn terminate(pid: u32) {
        // SAFETY: 打开失败（进程已经退出）时不使用句柄；打开的句柄用完就关。
        unsafe {
            let process = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !process.is_null() {
                TerminateProcess(process, 1);
                CloseHandle(process);
            }
        }
    }

    /// 有可见窗口的进程：用户能看到、可能正在用的程序。
    fn processes_with_windows() -> HashSet<u32> {
        unsafe extern "system" fn visit(window: HWND, found: LPARAM) -> i32 {
            // SAFETY: `found` 是下面传入的 HashSet 指针，EnumWindows 返回前一直有效，且只在本线程使用。
            unsafe {
                if IsWindowVisible(window) != 0 {
                    let mut pid = 0;
                    GetWindowThreadProcessId(window, &mut pid);
                    (*(found as *mut HashSet<u32>)).insert(pid);
                }
            }
            1
        }
        let mut found = HashSet::new();
        // SAFETY: 回调只访问 `found`，它在调用期间一直有效。
        unsafe { EnumWindows(Some(visit), &mut found as *mut HashSet<u32> as LPARAM) };
        found
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::process::{Command, Stdio};
    use std::thread;

    use super::*;
    use crate::process::spawn_tree;

    fn running(marker: &str) -> usize {
        let out = Command::new("ps").args(["-axo", "stat=,command="]).output().unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| {
                !l.trim_start().starts_with('Z') && l.contains(&format!("sleep {marker}")) && !l.contains("sh -c")
            })
            .count()
    }

    fn wait_for(marker: &str, count: usize) {
        for _ in 0..100 {
            if running(marker) == count {
                return;
            }
            thread::sleep(Duration::from_millis(30));
        }
        panic!("expected {count} processes, found {}", running(marker));
    }

    #[test]
    fn kills_wrapped_and_background_children() {
        // 和 npm 装的 codex 一样隔着一层；另外还有后台进程和忽略挂断信号的进程。
        let marker = "71.3301";
        let script =
            format!("sleep {marker} & (trap '' HUP TERM; sh -c 'sleep {marker}; true') & sh -c 'sleep {marker}; true'");
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &script]).stdout(Stdio::null());
        let (mut child, tree) = spawn_tree(&mut cmd).unwrap();
        wait_for(marker, 3);
        tree.kill(None, Duration::from_secs(2));
        child.wait().unwrap();
        wait_for(marker, 0);
    }

    #[test]
    fn remembers_children_after_the_leader_exits() {
        let marker = "71.3302";
        // perl 的 setsid 开一个新会话，像 Claude Code 跑命令那样。
        let script = format!(
            "perl -e 'use POSIX; if (fork == 0) {{ POSIX::setsid(); exec \"sleep\", \"{marker}\" }}; sleep 60'"
        );
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &format!("{script}; true")]);
        let (mut child, tree) = spawn_tree(&mut cmd).unwrap();
        wait_for(marker, 1);
        let members = tree.members();
        child.kill().unwrap();
        child.wait().unwrap();
        tree.kill(Some(members), Duration::from_secs(2));
        wait_for(marker, 0);
    }

    #[test]
    fn leaves_other_processes_alone() {
        let marker = "71.3303";
        let mut other = Command::new("sleep").arg(marker).spawn().unwrap();
        let (mut child, tree) = spawn_tree(Command::new("sleep").arg("60")).unwrap();
        wait_for(marker, 1);
        tree.kill(None, Duration::from_secs(2));
        child.wait().unwrap();
        // 主进程已经回收，再结束一次也不会误伤。
        tree.kill(None, Duration::from_millis(50));
        assert_eq!(running(marker), 1);
        other.kill().unwrap();
        other.wait().unwrap();
    }
}
