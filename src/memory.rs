#[cfg(any(target_os = "windows", test))]
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, WebviewWindow};

use crate::window::MAIN_WINDOW_LABEL;

#[cfg(target_os = "windows")]
use std::sync::atomic::AtomicU32;
#[cfg(target_os = "windows")]
use std::sync::Arc;
#[cfg(target_os = "windows")]
use webview2_com::{
    ClearBrowsingDataCompletedHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2Profile2, ICoreWebView2_13, ICoreWebView2_19, ICoreWebView2_3,
        COREWEBVIEW2_BROWSING_DATA_KINDS, COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE,
        COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    },
    TrySuspendCompletedHandler,
};
#[cfg(target_os = "windows")]
use windows::core::Interface;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Memory::{
    SetProcessWorkingSetSizeEx, SETPROCESSWORKINGSETSIZEEX_FLAGS,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, ProcessPowerThrottling, SetProcessInformation,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_STATE, PROCESS_SET_INFORMATION, PROCESS_SET_QUOTA,
};

pub const SMART_IDLE_LOW_MEMORY_DELAY_SECS: u64 = 30;
pub const SMART_IDLE_TRIM_DELAY_SECS: u64 = 45;

#[cfg(target_os = "windows")]
static WEBVIEW_BROWSER_PID: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAction {
    SetLow,
    SetNormal,
}

#[derive(Debug, Default)]
pub struct MinimizeStateTracker {
    was_minimized: AtomicBool,
}

impl MinimizeStateTracker {
    pub const fn new() -> Self {
        Self {
            was_minimized: AtomicBool::new(false),
        }
    }

    pub fn update(&self, is_minimized: bool) -> Option<MemoryAction> {
        let prev = self.was_minimized.swap(is_minimized, Ordering::AcqRel);
        if prev == is_minimized {
            return None;
        }
        if is_minimized {
            Some(MemoryAction::SetLow)
        } else {
            Some(MemoryAction::SetNormal)
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IdleEffects {
    pub memory_level: Option<MemoryAction>,
    pub eco_qos: Option<bool>,
    pub trim_working_set: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LifecycleMode {
    Active,
    UnfocusedWaitingLow { low_at: Instant, trim_at: Instant },
    UnfocusedWaitingTrim { trim_at: Instant },
    UnfocusedIdleDone,
    Minimized,
    HiddenToTray,
}

#[derive(Debug)]
pub(crate) struct IdlePolicyState {
    mode: LifecycleMode,
    low_memory_active: bool,
    eco_qos_active: bool,
}

impl IdlePolicyState {
    pub fn new() -> Self {
        Self {
            mode: LifecycleMode::Active,
            low_memory_active: false,
            eco_qos_active: false,
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        match self.mode {
            LifecycleMode::UnfocusedWaitingLow { low_at, .. } => Some(low_at),
            LifecycleMode::UnfocusedWaitingTrim { trim_at } => Some(trim_at),
            LifecycleMode::Active
            | LifecycleMode::UnfocusedIdleDone
            | LifecycleMode::Minimized
            | LifecycleMode::HiddenToTray => None,
        }
    }

    pub fn on_focused(&mut self) -> IdleEffects {
        self.mode = LifecycleMode::Active;
        let memory_level = if self.low_memory_active {
            self.low_memory_active = false;
            Some(MemoryAction::SetNormal)
        } else {
            None
        };
        let eco_qos = if self.eco_qos_active {
            self.eco_qos_active = false;
            Some(false)
        } else {
            None
        };
        IdleEffects {
            memory_level,
            eco_qos,
            trim_working_set: false,
        }
    }

    pub fn on_unfocused(&mut self, now: Instant) -> IdleEffects {
        if self.mode == LifecycleMode::Active {
            self.mode = LifecycleMode::UnfocusedWaitingLow {
                low_at: now + Duration::from_secs(SMART_IDLE_LOW_MEMORY_DELAY_SECS),
                trim_at: now + Duration::from_secs(SMART_IDLE_TRIM_DELAY_SECS),
            };
        }
        IdleEffects::default()
    }

    pub fn on_minimized(&mut self) -> IdleEffects {
        if matches!(
            self.mode,
            LifecycleMode::Minimized | LifecycleMode::HiddenToTray
        ) {
            return IdleEffects::default();
        }
        self.mode = LifecycleMode::Minimized;
        let memory_level = if !self.low_memory_active {
            self.low_memory_active = true;
            Some(MemoryAction::SetLow)
        } else {
            None
        };
        let eco_qos = if !self.eco_qos_active {
            self.eco_qos_active = true;
            Some(true)
        } else {
            None
        };
        IdleEffects {
            memory_level,
            eco_qos,
            trim_working_set: true,
        }
    }

    pub fn on_restored(&mut self) -> IdleEffects {
        if self.mode == LifecycleMode::HiddenToTray {
            return IdleEffects::default();
        }
        self.mode = LifecycleMode::Active;
        let memory_level = if self.low_memory_active {
            self.low_memory_active = false;
            Some(MemoryAction::SetNormal)
        } else {
            None
        };
        let eco_qos = if self.eco_qos_active {
            self.eco_qos_active = false;
            Some(false)
        } else {
            None
        };
        IdleEffects {
            memory_level,
            eco_qos,
            trim_working_set: false,
        }
    }

    pub fn on_hidden_to_tray(&mut self) -> IdleEffects {
        self.mode = LifecycleMode::HiddenToTray;
        let memory_level = if !self.low_memory_active {
            self.low_memory_active = true;
            Some(MemoryAction::SetLow)
        } else {
            None
        };
        let eco_qos = if !self.eco_qos_active {
            self.eco_qos_active = true;
            Some(true)
        } else {
            None
        };
        IdleEffects {
            memory_level,
            eco_qos,
            trim_working_set: false,
        }
    }

    pub fn on_suspend_completed(&mut self) -> IdleEffects {
        if self.mode == LifecycleMode::HiddenToTray {
            IdleEffects {
                memory_level: None,
                eco_qos: None,
                trim_working_set: true,
            }
        } else {
            IdleEffects::default()
        }
    }

    pub fn on_shown_from_tray(&mut self) -> IdleEffects {
        self.mode = LifecycleMode::Active;
        self.low_memory_active = false;
        let eco_qos = if self.eco_qos_active {
            self.eco_qos_active = false;
            Some(false)
        } else {
            None
        };
        IdleEffects {
            memory_level: None,
            eco_qos,
            trim_working_set: false,
        }
    }

    pub fn on_tick(&mut self, now: Instant) -> IdleEffects {
        match self.mode {
            LifecycleMode::UnfocusedWaitingLow { low_at, trim_at } if now >= low_at => {
                let mut effects = IdleEffects::default();
                if !self.low_memory_active {
                    self.low_memory_active = true;
                    effects.memory_level = Some(MemoryAction::SetLow);
                }
                if !self.eco_qos_active {
                    self.eco_qos_active = true;
                    effects.eco_qos = Some(true);
                }
                if now >= trim_at {
                    self.mode = LifecycleMode::UnfocusedIdleDone;
                    effects.trim_working_set = true;
                } else {
                    self.mode = LifecycleMode::UnfocusedWaitingTrim { trim_at };
                }
                effects
            }
            LifecycleMode::UnfocusedWaitingTrim { trim_at } if now >= trim_at => {
                self.mode = LifecycleMode::UnfocusedIdleDone;
                IdleEffects {
                    memory_level: None,
                    eco_qos: None,
                    trim_working_set: true,
                }
            }
            _ => IdleEffects::default(),
        }
    }
}

enum MemoryCommand {
    Focused,
    Unfocused,
    Minimized,
    Restored,
    HiddenToTray,
    SuspendCompleted,
    ShownFromTray,
    TrimWorkingSetNow,
    Shutdown,
}

pub struct MemoryManager {
    tx: Mutex<Option<mpsc::Sender<MemoryCommand>>>,
}

impl MemoryManager {
    pub fn start(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel::<MemoryCommand>();
        let spawn_result = std::thread::Builder::new()
            .name("ztik-memory-mgr".to_string())
            .spawn(move || run_memory_worker(app, rx));
        if let Err(err) = spawn_result {
            eprintln!("[ztik][memory] failed to spawn memory worker thread: {err}");
            return Self {
                tx: Mutex::new(None),
            };
        }
        Self {
            tx: Mutex::new(Some(tx)),
        }
    }

    fn send(&self, cmd: MemoryCommand) {
        let guard = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref tx) = *guard {
            let _ = tx.send(cmd);
        }
    }

    fn shutdown(&self) {
        let mut guard = self.tx.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tx) = guard.take() {
            let _ = tx.send(MemoryCommand::Shutdown);
        }
    }
}

fn run_memory_worker(app: AppHandle, rx: mpsc::Receiver<MemoryCommand>) {
    let mut policy = IdlePolicyState::new();
    loop {
        let recv_result = match policy.next_deadline() {
            Some(deadline) => {
                let wait = deadline.saturating_duration_since(Instant::now());
                rx.recv_timeout(wait)
            }
            None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };

        let effects = match recv_result {
            Ok(MemoryCommand::Focused) => policy.on_focused(),
            Ok(MemoryCommand::Unfocused) => policy.on_unfocused(Instant::now()),
            Ok(MemoryCommand::Minimized) => policy.on_minimized(),
            Ok(MemoryCommand::Restored) => policy.on_restored(),
            Ok(MemoryCommand::HiddenToTray) => policy.on_hidden_to_tray(),
            Ok(MemoryCommand::SuspendCompleted) => policy.on_suspend_completed(),
            Ok(MemoryCommand::ShownFromTray) => policy.on_shown_from_tray(),
            Ok(MemoryCommand::TrimWorkingSetNow) => IdleEffects {
                memory_level: None,
                eco_qos: None,
                trim_working_set: true,
            },
            Ok(MemoryCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => policy.on_tick(Instant::now()),
        };

        apply_idle_effects(&app, effects);
    }
}

fn apply_idle_effects(app: &AppHandle, effects: IdleEffects) {
    if let Some(enable_eco) = effects.eco_qos {
        apply_process_tree_eco_qos(enable_eco);
    }

    if let Some(level) = effects.memory_level {
        let app_handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(win) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
                match level {
                    MemoryAction::SetLow => set_low_memory(&win),
                    MemoryAction::SetNormal => set_normal_memory(&win),
                }
            }
        });
    }

    if effects.trim_working_set {
        trim_process_tree_working_set();
    }
}

pub fn notify_focused(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.send(MemoryCommand::Focused);
    }
}

pub fn notify_unfocused(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.send(MemoryCommand::Unfocused);
    }
}

pub fn notify_minimized(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.send(MemoryCommand::Minimized);
    }
}

pub fn notify_restored(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.send(MemoryCommand::Restored);
    }
}

pub fn notify_hidden_to_tray(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.send(MemoryCommand::HiddenToTray);
    }
}

pub fn notify_suspend_completed(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.send(MemoryCommand::SuspendCompleted);
    }
}

pub fn notify_shown_from_tray(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.send(MemoryCommand::ShownFromTray);
    }
}

pub fn trim_working_set_now(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.send(MemoryCommand::TrimWorkingSetNow);
    }
}

pub fn shutdown(app: &AppHandle) {
    if let Some(mgr) = app.try_state::<MemoryManager>() {
        mgr.shutdown();
    }
}

pub fn record_webview_browser_pid(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    record_webview_browser_pid_windows(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn set_low_memory(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    set_memory_level(window, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, "Low");
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn set_normal_memory(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    set_memory_level(
        window,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        "Normal",
    );
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn set_webview_visible(window: &WebviewWindow, visible: bool) {
    #[cfg(target_os = "windows")]
    set_webview_visible_windows(window, visible);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, visible);
}

pub fn suspend(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    suspend_windows(window);
    #[cfg(not(target_os = "windows"))]
    {
        notify_suspend_completed(window.app_handle());
    }
}

pub fn resume(window: &WebviewWindow) {
    notify_shown_from_tray(window.app_handle());
    #[cfg(target_os = "windows")]
    resume_windows(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn clear_safe_temp_cache<F>(window: &WebviewWindow, on_complete: F)
where
    F: FnOnce() + Send + 'static,
{
    #[cfg(target_os = "windows")]
    clear_safe_temp_cache_windows(window, on_complete);
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        on_complete();
    }
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn collect_descendant_pids(root_pids: &[u32], entries: &[(u32, u32)]) -> Vec<u32> {
    let mut children_by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(pid, parent_pid) in entries {
        if pid == 0 || pid == parent_pid {
            continue;
        }
        children_by_parent.entry(parent_pid).or_default().push(pid);
    }

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut result = Vec::new();

    for &root in root_pids {
        if root != 0 && visited.insert(root) {
            queue.push_back(root);
            result.push(root);
        }
    }

    while let Some(current) = queue.pop_front() {
        if let Some(children) = children_by_parent.get(&current) {
            for &child in children {
                if visited.insert(child) {
                    queue.push_back(child);
                    result.push(child);
                }
            }
        }
    }

    result
}

#[cfg(target_os = "windows")]
struct OwnedHandle(HANDLE);

#[cfg(target_os = "windows")]
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(target_os = "windows")]
fn exe_name_matches_webview2(raw: &[u16]) -> bool {
    let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    let name = String::from_utf16_lossy(&raw[..len]);
    name.eq_ignore_ascii_case("msedgewebview2.exe")
}

#[cfg(target_os = "windows")]
pub(crate) fn collect_ztik_process_tree_pids() -> Vec<u32> {
    let current_pid = std::process::id();
    let recorded_browser_pid = WEBVIEW_BROWSER_PID.load(Ordering::Acquire);

    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(handle) => OwnedHandle(handle),
        Err(err) => {
            eprintln!("[ztik][memory] CreateToolhelp32Snapshot failed: {err}");
            return vec![current_pid];
        }
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut pairs = Vec::new();
    let mut browser_pid_verified = false;

    if unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_ok() {
        loop {
            let pid = entry.th32ProcessID;
            let parent_pid = entry.th32ParentProcessID;
            pairs.push((pid, parent_pid));
            if recorded_browser_pid != 0
                && pid == recorded_browser_pid
                && exe_name_matches_webview2(&entry.szExeFile)
            {
                browser_pid_verified = true;
            }
            if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
    }

    let mut roots = Vec::with_capacity(2);
    roots.push(current_pid);
    if browser_pid_verified && recorded_browser_pid != current_pid {
        roots.push(recorded_browser_pid);
    }

    collect_descendant_pids(&roots, &pairs)
}

#[cfg(target_os = "windows")]
fn set_process_eco_qos(handle: HANDLE, enable: bool) -> bool {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: if enable {
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
    };
    unsafe {
        SetProcessInformation(
            handle,
            ProcessPowerThrottling,
            (&state as *const PROCESS_POWER_THROTTLING_STATE).cast(),
            std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
        .is_ok()
    }
}

#[cfg(target_os = "windows")]
fn apply_process_tree_eco_qos(enable: bool) {
    let current_pid = std::process::id();
    let pids = collect_ztik_process_tree_pids();
    let mut updated = 0usize;

    for pid in pids {
        if pid == current_pid {
            if set_process_eco_qos(unsafe { GetCurrentProcess() }, enable) {
                updated += 1;
            }
            continue;
        }
        if let Ok(handle) = unsafe { OpenProcess(PROCESS_SET_INFORMATION, false, pid) } {
            let _guard = OwnedHandle(handle);
            if set_process_eco_qos(handle, enable) {
                updated += 1;
            }
        }
    }

    eprintln!(
        "[ztik][memory] EcoQoS {} for {updated} process(es)",
        if enable { "enabled" } else { "disabled" }
    );
}

#[cfg(not(target_os = "windows"))]
fn apply_process_tree_eco_qos(_enable: bool) {}

#[cfg(target_os = "windows")]
fn trim_process_working_set(handle: HANDLE) -> bool {
    unsafe {
        SetProcessWorkingSetSizeEx(
            handle,
            usize::MAX,
            usize::MAX,
            SETPROCESSWORKINGSETSIZEEX_FLAGS(0),
        )
        .is_ok()
    }
}

#[cfg(target_os = "windows")]
fn trim_process_tree_working_set() {
    let current_pid = std::process::id();
    let pids = collect_ztik_process_tree_pids();
    let mut trimmed = 0usize;

    for pid in pids {
        if pid == current_pid {
            if trim_process_working_set(unsafe { GetCurrentProcess() }) {
                trimmed += 1;
            }
            continue;
        }
        if let Ok(handle) = unsafe { OpenProcess(PROCESS_SET_QUOTA, false, pid) } {
            let _guard = OwnedHandle(handle);
            if trim_process_working_set(handle) {
                trimmed += 1;
            }
        }
    }

    eprintln!("[ztik][memory] trimmed working set for {trimmed} process(es)");
}

#[cfg(not(target_os = "windows"))]
fn trim_process_tree_working_set() {}

#[cfg(target_os = "windows")]
pub(crate) fn safe_temp_cache_data_kinds() -> COREWEBVIEW2_BROWSING_DATA_KINDS {
    // Strictly temporary HTTP/CacheStorage caches only — never Cookies, LocalStorage, or IndexedDB.
    COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE
}

#[cfg(target_os = "windows")]
fn record_webview_browser_pid_windows(window: &WebviewWindow) {
    let _ = window.with_webview(|platform_webview| {
        if let Ok(core_webview) = unsafe { platform_webview.controller().CoreWebView2() } {
            let mut pid = 0u32;
            if unsafe { core_webview.BrowserProcessId(&mut pid) }.is_ok() && pid != 0 {
                WEBVIEW_BROWSER_PID.store(pid, Ordering::Release);
            }
        }
    });
}

#[cfg(target_os = "windows")]
type CompletionSlot = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

#[cfg(target_os = "windows")]
fn run_completion_once(slot: &CompletionSlot) {
    if let Some(callback) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() {
        callback();
    }
}

#[cfg(target_os = "windows")]
fn clear_safe_temp_cache_windows<F>(window: &WebviewWindow, on_complete: F)
where
    F: FnOnce() + Send + 'static,
{
    let completion: CompletionSlot = Arc::new(Mutex::new(Some(Box::new(on_complete))));

    let completion_for_webview = Arc::clone(&completion);
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        let Ok(core_webview) = (unsafe { controller.CoreWebView2() }) else {
            run_completion_once(&completion_for_webview);
            return;
        };
        let Ok(webview13) = core_webview.cast::<ICoreWebView2_13>() else {
            eprintln!("[ztik][memory] ICoreWebView2_13 unavailable, skipping disk cache clear");
            run_completion_once(&completion_for_webview);
            return;
        };
        let Ok(profile) = (unsafe { webview13.Profile() }) else {
            run_completion_once(&completion_for_webview);
            return;
        };
        let Ok(profile2) = profile.cast::<ICoreWebView2Profile2>() else {
            eprintln!(
                "[ztik][memory] ICoreWebView2Profile2 unavailable, skipping disk cache clear"
            );
            run_completion_once(&completion_for_webview);
            return;
        };

        let completion_for_cb = Arc::clone(&completion_for_webview);
        let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |result| {
            if let Err(err) = result {
                eprintln!("[ztik][memory] ClearBrowsingData(temp cache) error: {err}");
            } else {
                eprintln!(
                    "[ztik][memory] cleared WebView2 temporary cache (preserved session storage)"
                );
            }
            run_completion_once(&completion_for_cb);
            Ok(())
        }));

        if let Err(err) =
            unsafe { profile2.ClearBrowsingData(safe_temp_cache_data_kinds(), &handler) }
        {
            eprintln!("[ztik][memory] ClearBrowsingData call failed: {err}");
            run_completion_once(&completion_for_webview);
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for cache clear: {err}");
        run_completion_once(&completion);
    }
}

#[cfg(target_os = "windows")]
fn apply_memory_target_level(
    core_webview: &ICoreWebView2,
    level: COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
    level_name: &'static str,
) {
    match core_webview.cast::<ICoreWebView2_19>() {
        Ok(webview19) => {
            if let Err(err) = unsafe { webview19.SetMemoryUsageTargetLevel(level) } {
                eprintln!("[ztik][memory] SetMemoryUsageTargetLevel({level_name}) failed: {err}");
            } else {
                eprintln!("[ztik][memory] MemoryUsageTargetLevel -> {level_name}");
            }
        }
        Err(err) => {
            eprintln!("[ztik][memory] ICoreWebView2_19 unavailable, skipping {level_name}: {err}");
        }
    }
}

#[cfg(target_os = "windows")]
fn set_memory_level(
    window: &WebviewWindow,
    level: COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
    level_name: &'static str,
) {
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        match unsafe { controller.CoreWebView2() } {
            Ok(core_webview) => apply_memory_target_level(&core_webview, level, level_name),
            Err(err) => eprintln!("[ztik][memory] CoreWebView2() failed: {err}"),
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for {level_name}: {err}");
    }
}

#[cfg(target_os = "windows")]
fn set_webview_visible_windows(window: &WebviewWindow, visible: bool) {
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        if let Err(err) = unsafe { controller.SetIsVisible(visible) } {
            eprintln!("[ztik][memory] SetIsVisible({visible}) failed: {err}");
        } else {
            eprintln!("[ztik][memory] SetIsVisible({visible})");
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for SetIsVisible({visible}): {err}");
    }
}

#[cfg(target_os = "windows")]
fn suspend_windows(window: &WebviewWindow) {
    let app_handle = window.app_handle().clone();
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        if let Err(err) = unsafe { controller.SetIsVisible(false) } {
            eprintln!("[ztik][memory] SetIsVisible(false) failed: {err}");
            return;
        }

        let core_webview = match unsafe { controller.CoreWebView2() } {
            Ok(webview) => webview,
            Err(err) => {
                eprintln!("[ztik][memory] CoreWebView2() failed during suspend: {err}");
                notify_suspend_completed(&app_handle);
                return;
            }
        };

        let mut pid = 0u32;
        if unsafe { core_webview.BrowserProcessId(&mut pid) }.is_ok() && pid != 0 {
            WEBVIEW_BROWSER_PID.store(pid, Ordering::Release);
        }

        apply_memory_target_level(
            &core_webview,
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            "Low",
        );

        match core_webview.cast::<ICoreWebView2_3>() {
            Ok(webview3) => {
                let app_for_cb = app_handle.clone();
                let handler =
                    TrySuspendCompletedHandler::create(Box::new(move |result, is_successful| {
                        if let Err(err) = result {
                            eprintln!("[ztik][memory] TrySuspend callback error: {err}");
                        } else if !is_successful {
                            eprintln!(
                                "[ztik][memory] TrySuspend completed with is_successful=false"
                            );
                        } else {
                            eprintln!("[ztik][memory] WebView2 suspended");
                        }
                        notify_suspend_completed(&app_for_cb);
                        Ok(())
                    }));
                if let Err(err) = unsafe { webview3.TrySuspend(&handler) } {
                    eprintln!("[ztik][memory] TrySuspend failed: {err}");
                    notify_suspend_completed(&app_handle);
                } else {
                    eprintln!("[ztik][memory] TrySuspend requested");
                }
            }
            Err(err) => {
                eprintln!("[ztik][memory] ICoreWebView2_3 unavailable, skipping TrySuspend: {err}");
                notify_suspend_completed(&app_handle);
            }
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for suspend: {err}");
    }
}

#[cfg(target_os = "windows")]
fn resume_windows(window: &WebviewWindow) {
    if let Err(err) = window.with_webview(move |platform_webview| {
        let controller = platform_webview.controller();
        let core_webview = match unsafe { controller.CoreWebView2() } {
            Ok(webview) => Some(webview),
            Err(err) => {
                eprintln!("[ztik][memory] CoreWebView2() failed during resume: {err}");
                None
            }
        };

        if let Some(ref webview) = core_webview {
            match webview.cast::<ICoreWebView2_3>() {
                Ok(webview3) => {
                    if let Err(err) = unsafe { webview3.Resume() } {
                        eprintln!("[ztik][memory] Resume failed: {err}");
                    } else {
                        eprintln!("[ztik][memory] WebView2 resumed");
                    }
                }
                Err(err) => {
                    eprintln!("[ztik][memory] ICoreWebView2_3 unavailable, skipping Resume: {err}");
                }
            }
        }

        if let Err(err) = unsafe { controller.SetIsVisible(true) } {
            eprintln!("[ztik][memory] SetIsVisible(true) failed: {err}");
        } else {
            eprintln!("[ztik][memory] SetIsVisible(true)");
        }

        if let Some(ref webview) = core_webview {
            apply_memory_target_level(
                webview,
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
                "Normal",
            );
        }
    }) {
        eprintln!("[ztik][memory] with_webview failed for resume: {err}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        collect_descendant_pids, IdleEffects, IdlePolicyState, MemoryAction, MinimizeStateTracker,
        SMART_IDLE_LOW_MEMORY_DELAY_SECS, SMART_IDLE_TRIM_DELAY_SECS,
    };

    #[test]
    fn transitions_only_when_minimized_state_changes() {
        let tracker = MinimizeStateTracker::new();

        // Repeated non-minimized events on startup or normal resize/focus do nothing.
        assert_eq!(tracker.update(false), None);
        assert_eq!(tracker.update(false), None);

        // Minimize triggers SetLow exactly once.
        assert_eq!(tracker.update(true), Some(MemoryAction::SetLow));
        assert_eq!(tracker.update(true), None);
        assert_eq!(tracker.update(true), None);

        // Restore triggers SetNormal exactly once.
        assert_eq!(tracker.update(false), Some(MemoryAction::SetNormal));
        assert_eq!(tracker.update(false), None);

        // Second cycle works identically.
        assert_eq!(tracker.update(true), Some(MemoryAction::SetLow));
        assert_eq!(tracker.update(false), Some(MemoryAction::SetNormal));
    }

    #[test]
    fn collects_only_ztik_process_tree_descendants_without_other_apps() {
        let entries = vec![
            (0, 0),     // System idle self-loop
            (100, 1),   // ztik.exe
            (200, 100), // ztik WebView2 browser process
            (201, 200), // ztik WebView2 GPU process
            (202, 200), // ztik WebView2 TikTok renderer
            (203, 202), // nested utility child
            (500, 1),   // unrelated app (e.g. Teams)
            (501, 500), // unrelated app's WebView2 process
            (502, 501), // unrelated app's WebView2 renderer
            (900, 900), // synthetic self-cycle
        ];

        let pids = collect_descendant_pids(&[100], &entries);
        assert_eq!(pids, vec![100, 200, 201, 202, 203]);
        assert!(!pids.contains(&500));
        assert!(!pids.contains(&501));
        assert!(!pids.contains(&502));
    }

    #[test]
    fn smart_idle_cancels_before_30s_and_triggers_low_eco_at_30s_then_trim_at_45s() {
        assert_eq!(SMART_IDLE_LOW_MEMORY_DELAY_SECS, 30);
        assert_eq!(SMART_IDLE_TRIM_DELAY_SECS, 45);

        let t0 = Instant::now();
        let mut policy = IdlePolicyState::new();

        // Losing focus schedules 30s timer without throttling immediately.
        assert_eq!(policy.on_unfocused(t0), IdleEffects::default());
        assert_eq!(
            policy.next_deadline(),
            Some(t0 + Duration::from_secs(SMART_IDLE_LOW_MEMORY_DELAY_SECS))
        );

        // Refocusing at 15s cancels the timer cleanly with zero OS/WebView calls.
        assert_eq!(policy.on_focused(), IdleEffects::default());
        assert_eq!(policy.next_deadline(), None);

        // Lose focus again and cross 30s -> Low memory + EcoQoS ON, schedules 45s trim.
        assert_eq!(policy.on_unfocused(t0), IdleEffects::default());
        assert_eq!(
            policy.on_tick(t0 + Duration::from_secs(29)),
            IdleEffects::default()
        );
        assert_eq!(
            policy.on_tick(t0 + Duration::from_secs(30)),
            IdleEffects {
                memory_level: Some(MemoryAction::SetLow),
                eco_qos: Some(true),
                trim_working_set: false,
            }
        );
        assert_eq!(
            policy.next_deadline(),
            Some(t0 + Duration::from_secs(SMART_IDLE_TRIM_DELAY_SECS))
        );

        // Cross 45s -> Trim Working Set once, no more pending timers.
        assert_eq!(
            policy.on_tick(t0 + Duration::from_secs(45)),
            IdleEffects {
                memory_level: None,
                eco_qos: None,
                trim_working_set: true,
            }
        );
        assert_eq!(policy.next_deadline(), None);

        // Returning focus immediately disables EcoQoS and restores Normal memory.
        assert_eq!(
            policy.on_focused(),
            IdleEffects {
                memory_level: Some(MemoryAction::SetNormal),
                eco_qos: Some(false),
                trim_working_set: false,
            }
        );
        // Subsequent focus events are no-ops.
        assert_eq!(policy.on_focused(), IdleEffects::default());
    }

    #[test]
    fn minimize_and_tray_lifecycle_triggers_eco_qos_and_working_set_trim() {
        let mut policy = IdlePolicyState::new();

        assert_eq!(
            policy.on_minimized(),
            IdleEffects {
                memory_level: Some(MemoryAction::SetLow),
                eco_qos: Some(true),
                trim_working_set: true,
            }
        );
        assert_eq!(
            policy.on_restored(),
            IdleEffects {
                memory_level: Some(MemoryAction::SetNormal),
                eco_qos: Some(false),
                trim_working_set: false,
            }
        );

        // Hide to tray enables EcoQoS + Low, then trims after TrySuspend completes.
        assert_eq!(
            policy.on_hidden_to_tray(),
            IdleEffects {
                memory_level: Some(MemoryAction::SetLow),
                eco_qos: Some(true),
                trim_working_set: false,
            }
        );
        assert_eq!(
            policy.on_suspend_completed(),
            IdleEffects {
                memory_level: None,
                eco_qos: None,
                trim_working_set: true,
            }
        );

        // Show from tray disables EcoQoS immediately (resume_windows restores Normal).
        assert_eq!(
            policy.on_shown_from_tray(),
            IdleEffects {
                memory_level: None,
                eco_qos: Some(false),
                trim_working_set: false,
            }
        );
        // Stale suspend callback after restore must not trim.
        assert_eq!(policy.on_suspend_completed(), IdleEffects::default());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn temp_cache_clear_never_deletes_session_storage_or_cookies() {
        use super::safe_temp_cache_data_kinds;
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE,
            COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE,
            COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE, COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES,
            COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE,
            COREWEBVIEW2_BROWSING_DATA_KINDS_INDEXED_DB,
            COREWEBVIEW2_BROWSING_DATA_KINDS_LOCAL_STORAGE,
        };

        let kinds = safe_temp_cache_data_kinds();
        assert!(kinds.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE));
        assert!(!kinds.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES));
        assert!(!kinds.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_LOCAL_STORAGE));
        assert!(!kinds.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_INDEXED_DB));
        assert!(!kinds.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE));
        assert!(!kinds.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE));
        assert!(!kinds.contains(COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE));
    }
}
