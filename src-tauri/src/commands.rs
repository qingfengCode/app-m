use crate::models::{
    AddAppParams, AppConfig, AppInstance, AppState, AppType, CommandResult, LogEntry, MetricPoint,
    PersistData, ProcessInfo, StaticServerConfig, SystemInfo, UpdateAppParams, UpdateSortOrderParams,
};
use crate::static_server;
use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event, EventKind, Config as NotifyConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessesToUpdate};
use tauri::AppHandle;
use tauri::Manager;
use tauri::State;

fn get_data_file_path(state: &State<'_, AppState>) -> String {
    let dir = state.app_data_dir.lock().unwrap().clone();
    std::path::Path::new(&dir)
        .join("apps.json")
        .to_string_lossy()
        .to_string()
}

fn save_apps_to_disk(state: &State<'_, AppState>) {
    let path = get_data_file_path(state);
    let apps = state.apps.lock().unwrap();
    let mut instances: Vec<AppInstance> = apps.values().map(|i| {
        let mut inst = i.clone();
        inst.process_info = None;
        inst.logs = None;
        inst
    }).collect();
    instances.sort_by_key(|a| a.config.sort_order);
    let data = PersistData { apps: instances };
    if let Ok(json) = serde_json::to_string_pretty(&data) {
        let _ = fs::write(&path, json);
    }
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

const MAX_LOGS: usize = 2000;

fn push_log(logs: &mut Vec<LogEntry>, level: &str, content: &str) {
    logs.push(LogEntry {
        timestamp: now_ts(),
        level: level.to_string(),
        content: content.to_string(),
    });
    if logs.len() > MAX_LOGS {
        logs.drain(0..logs.len() - MAX_LOGS);
    }
}

fn push_to_buffer(log_buffers: &Mutex<HashMap<String, Arc<Mutex<Vec<LogEntry>>>>>, app_id: &str, level: &str, content: &str) {
    let buffers = log_buffers.lock().unwrap();
    if let Some(buf) = buffers.get(app_id) {
        push_log(&mut buf.lock().unwrap(), level, content);
    }
}

fn build_command(instance: &AppInstance) -> Command {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", &instance.config.command]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", &instance.config.command]);
        c
    };

    if let Some(ref work_dir) = instance.config.work_dir {
        if !work_dir.is_empty() {
            cmd.current_dir(work_dir);
        }
    }

    if let Some(ref env_vars) = instance.config.env_vars {
        for (key, value) in env_vars {
            cmd.env(key, value);
        }
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    cmd
}

/// Windows 下 `cmd /C <command>` 会先启动 cmd.exe 再派生子进程，
/// 监控到的进程其实是 cmd 包装进程。这里轮询解析出真实的子进程 PID（取树中最深的叶子）。
#[cfg(target_os = "windows")]
fn resolve_real_process(root_pid: u32) -> u32 {
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut system = sysinfo::System::new();
    let mut best = root_pid;
    let mut best_depth = 0;
    let mut stable = 0;
    let mut last_leaf = root_pid;

    while Instant::now() < deadline {
        system.refresh_processes(ProcessesToUpdate::All, true);

        // 从 root 向下走到最深的子进程
        let mut cur = root_pid;
        let mut depth = 0;
        loop {
            let root_ref = Pid::from_u32(cur);
            let mut children: Vec<u32> = system
                .processes()
                .values()
                .filter(|p| p.parent() == Some(root_ref))
                .map(|p| p.pid().as_u32())
                .collect();
            if children.is_empty() {
                break;
            }
            // 多个子进程时取 PID 最大的（较晚创建的）
            children.sort_unstable();
            cur = *children.last().unwrap();
            depth += 1;
            if depth > 10 {
                break;
            }
        }

        if depth > best_depth {
            best_depth = depth;
            best = cur;
        }

        if depth >= 1 {
            if cur == last_leaf {
                stable += 1;
                if stable >= 2 {
                    break;
                }
            } else {
                stable = 0;
                last_leaf = cur;
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }

    if best_depth >= 1 {
        best
    } else {
        root_pid
    }
}

#[cfg(not(target_os = "windows"))]
fn resolve_real_process(root_pid: u32) -> u32 {
    root_pid
}

fn spawn_and_track(
    instance: &mut AppInstance,
    tag: &str,
    children: &Mutex<HashMap<String, std::process::Child>>,
    log_buffers: &Mutex<HashMap<String, Arc<Mutex<Vec<LogEntry>>>>>,
    job_object: &crate::job_object::JobObject,
) -> bool {
    let mut cmd = build_command(instance);
    let app_id = instance.config.id.clone();
    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            let real_pid = resolve_real_process(pid);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            instance.pid = Some(real_pid);
            instance.running = true;
            instance.process_info = None;
            instance.started_at = Some(now);
            instance.exit_reason = None;
            instance.manual_stop = false;

            if let Err(e) = job_object.assign_process(&mut child) {
                eprintln!("警告: 无法将进程 {} 分配到 Job Object: {}", pid, e);
            }

            let log_msg = if tag.is_empty() {
                if real_pid != pid {
                    format!("启动成功，PID: {}（包装进程 {}）", real_pid, pid)
                } else {
                    format!("启动成功，PID: {}", pid)
                }
            } else if real_pid != pid {
                format!("{}启动成功，PID: {}（包装进程 {}）", tag, real_pid, pid)
            } else {
                format!("{}启动成功，PID: {}", tag, pid)
            };
            push_to_buffer(log_buffers, &app_id, "info", &log_msg);

            if let Some(stdout) = child.stdout.take() {
                let buf = log_buffers.lock().unwrap().get(&app_id).cloned();
                if let Some(log_buf) = buf {
                    std::thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines().flatten() {
                            push_log(&mut log_buf.lock().unwrap(), "info", &line);
                        }
                    });
                }
            }

            if let Some(stderr) = child.stderr.take() {
                let buf = log_buffers.lock().unwrap().get(&app_id).cloned();
                if let Some(log_buf) = buf {
                    std::thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines().flatten() {
                            push_log(&mut log_buf.lock().unwrap(), "error", &line);
                        }
                    });
                }
            }

            children.lock().unwrap().insert(app_id, child);
            true
        }
        Err(e) => {
            let msg = format!("{}启动失败: {}", tag, e);
            push_to_buffer(log_buffers, &app_id, "error", &msg);
            false
        }
    }
}

pub fn kill_tree(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(0x08000000)
            .output();
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
}

/// 仅终止单个进程（不递归树杀）：
/// Windows 下等价于 Task Manager 的"结束进程"，Unix 下发送 SIGTERM 优雅退出
fn kill_process(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .creation_flags(0x08000000)
            .output();
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("kill").args([&pid.to_string()]).output();
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
}

/// 优雅关闭：向进程发送关闭信号（不强制）
/// Windows 下 taskkill 不带 /F 会向进程窗口发送 WM_CLOSE；Unix 下 kill 发送 SIGTERM
fn kill_gracefully(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string()])
            .creation_flags(0x08000000)
            .output();
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("kill").args([&pid.to_string()]).output();
        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }
}

/// 判断指定 PID 的进程是否仍然存活
fn is_process_alive(pid: u32) -> bool {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), false);
    sys.process(Pid::from_u32(pid)).is_some()
}

fn kill_child(
    children: &Mutex<HashMap<String, std::process::Child>>,
    id: &str,
    force: bool,
    real_pid: Option<u32>,
) -> bool {
    let mut children = children.lock().unwrap();
    if let Some(child) = children.get_mut(id) {
        let pid = child.id();
        if force {
            // 强制关闭：递归终止整棵进程树
            let tree_killed = kill_tree(pid);
            let _ = child.kill();
            tree_killed
        } else {
            // 关闭：先向真实进程发送优雅关闭信号并等待其退出；
            // 超时未退出或无法发送信号时，回退为强制终止单个进程（不递归树杀）
            let target = real_pid.unwrap_or(pid);
            if kill_gracefully(target) {
                let deadline = Instant::now() + Duration::from_millis(1500);
                while Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(150));
                    if !is_process_alive(target) {
                        return true;
                    }
                }
            }
            let _ = child.kill();
            let _ = kill_process(target);
            !is_process_alive(target)
        }
    } else {
        false
    }
}

fn stop_file_watcher(state: &State<'_, AppState>, id: &str) {
    state.file_watchers.lock().unwrap().remove(id);
}

fn start_file_watcher(state: &State<'_, AppState>, app: &AppHandle, id: &str) {
    let (dirs, app_id) = {
        let apps = state.apps.lock().unwrap();
        if let Some(instance) = apps.get(id) {
            if !instance.config.watch_restart {
                return;
            }
            let mut dirs_to_watch = Vec::new();
            if let Some(ref watch_dirs) = instance.config.watch_dirs {
                for d in watch_dirs {
                    if !d.trim().is_empty() {
                        dirs_to_watch.push(d.trim().to_string());
                    }
                }
            }
            if dirs_to_watch.is_empty() {
                if let Some(ref work_dir) = instance.config.work_dir {
                    if !work_dir.trim().is_empty() {
                        dirs_to_watch.push(work_dir.trim().to_string());
                    }
                }
            }
            if dirs_to_watch.is_empty() {
                return;
            }
            (dirs_to_watch, id.to_string())
        } else {
            return;
        }
    };

    let (tx, rx) = mpsc::channel::<Result<Event, notify::Error>>();

    let mut watcher: RecommendedWatcher = match RecommendedWatcher::new(
        move |res| {
            let _ = tx.send(res);
        },
        NotifyConfig::default().with_poll_interval(Duration::from_secs(2)),
    ) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("文件监控创建失败: {}", e);
            return;
        }
    };

    for dir in &dirs {
        let path = std::path::Path::new(dir);
        if path.exists() {
            if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
                eprintln!("监控目录失败 {}: {}", dir, e);
            }
        }
    }

    state.file_watchers.lock().unwrap().insert(id.to_string(), watcher);

    // 通过 AppHandle 在子线程中重新获取 State，避免使用 unsafe 延长生命周期
    let app_handle = app.clone();
    let watch_id = app_id.clone();
    std::thread::spawn(move || {
        let state = app_handle.state::<AppState>();
        let mut last_restart = std::time::Instant::now();
        let debounce = Duration::from_secs(2);
        loop {
            match rx.recv_timeout(Duration::from_secs(30)) {
                Ok(Ok(event)) => {
                    if matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)) {
                        if last_restart.elapsed() < debounce {
                            continue;
                        }
                        last_restart = std::time::Instant::now();

                        let apps_map = state.apps.lock().unwrap();
                        if let Some(instance) = apps_map.get(&watch_id) {
                            if !instance.running {
                                drop(apps_map);
                                continue;
                            }
                        } else {
                            drop(apps_map);
                            break;
                        }
                        drop(apps_map);

                        let app_type = {
                            let apps_map = state.apps.lock().unwrap();
                            apps_map.get(&watch_id).map(|i| i.config.app_type.clone())
                        };
                        let Some(app_type) = app_type else { break };

                        match app_type {
                            AppType::StaticServer => {
                                {
                                    let mut apps_map = state.apps.lock().unwrap();
                                    if let Some(inst) = apps_map.get_mut(&watch_id) {
                                        push_to_buffer(&state.log_buffers, &watch_id, "info", "文件变更，准备自动重启...");
                                        inst.running = false;
                                        inst.pid = None;
                                        inst.process_info = None;
                                        inst.started_at = None;
                                        inst.server_port = None;
                                        inst.exit_reason = None;
                                    }
                                }
                                std::thread::sleep(Duration::from_millis(300));
                            }
                            AppType::Command => {
                                {
                                    let mut apps_map = state.apps.lock().unwrap();
                                    if let Some(inst) = apps_map.get_mut(&watch_id) {
                                        push_to_buffer(&state.log_buffers, &watch_id, "info", "文件变更，准备自动重启...");
                                        inst.running = false;
                                        inst.pid = None;
                                        inst.process_info = None;
                                        inst.started_at = None;
                                        inst.exit_reason = Some("文件变更自动重启".to_string());
                                    }
                                }
                                kill_child(&state.children, &watch_id, true, None);
                                state.children.lock().unwrap().remove(&watch_id);
                                std::thread::sleep(Duration::from_millis(500));
                            }
                        }

                        match app_type {
                            AppType::Command => {
                                let mut apps_map = state.apps.lock().unwrap();
                                if let Some(inst) = apps_map.get_mut(&watch_id) {
                                    if spawn_and_track(inst, "自动重启", &state.children, &state.log_buffers, &state.job_object) {
                                        push_to_buffer(&state.log_buffers, &watch_id, "info", "文件变更自动重启成功");
                                    } else {
                                        push_to_buffer(&state.log_buffers, &watch_id, "error", "文件变更自动重启失败");
                                    }
                                }
                            }
                            AppType::StaticServer => {
                                let sc_clone;
                                let port;
                                {
                                    let apps_map = state.apps.lock().unwrap();
                                    if let Some(inst) = apps_map.get(&watch_id) {
                                        if let Some(ref sc) = inst.config.static_server {
                                            sc_clone = sc.clone();
                                            port = sc.port;
                                        } else {
                                            continue;
                                        }
                                    } else {
                                        break;
                                    }
                                }
                                match tauri::async_runtime::block_on(async {
                                    static_server::start_static_server(&sc_clone).await
                                }) {
                                    Ok(abort_handle) => {
                                        state.running_servers.lock().unwrap().insert(watch_id.clone(), abort_handle);
                                        let mut apps_map = state.apps.lock().unwrap();
                                        if let Some(inst) = apps_map.get_mut(&watch_id) {
                                            inst.running = true;
                                            inst.server_port = Some(port);
                                            inst.started_at = Some(now_ts());
                                            push_to_buffer(&state.log_buffers, &watch_id, "info", &format!("文件变更自动重启成功，端口: {}", port));
                                        }
                                    }
                                    Err(e) => {
                                        let apps_map = state.apps.lock().unwrap();
                                        if apps_map.contains_key(&watch_id) {
                                            push_to_buffer(&state.log_buffers, &watch_id, "error", &format!("文件变更自动重启失败: {}", e));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Err(_)) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let apps_map = state.apps.lock().unwrap();
                    if !apps_map.contains_key(&watch_id) {
                        break;
                    }
                    drop(apps_map);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        state.file_watchers.lock().unwrap().remove(&watch_id);
    });
}

#[tauri::command]
pub fn add_app(
    state: State<'_, AppState>,
    params: AddAppParams,
) -> Result<CommandResult<AppConfig>, String> {
    let app_type = params.app_type.clone().unwrap_or(AppType::Command);
    if params.name.is_empty() {
        return Ok(CommandResult::error("名称不能为空"));
    }
    if matches!(app_type, AppType::Command) && params.command.is_empty() {
        return Ok(CommandResult::error("命令不能为空"));
    }
    if matches!(app_type, AppType::StaticServer) {
        if params.static_server.is_none() {
            return Ok(CommandResult::error("静态服务器配置不能为空"));
        }
        let sc = params.static_server.as_ref().unwrap();
        if sc.root_dir.is_empty() {
            return Ok(CommandResult::error("静态文件目录不能为空"));
        }
    }

    let sort_order = state.alloc_sort_order();
    let config = crate::models::create_app_config(params, sort_order);
    let config_clone = config.clone();

    let instance = AppInstance {
        config: config_clone,
        pid: None,
        running: false,
        process_info: None,
        started_at: None,
        logs: None,
        server_port: None,
        exit_reason: None,
        manual_stop: false,
    };

    let id = config.id.clone();
    state
        .apps
        .lock()
        .unwrap()
        .insert(id.clone(), instance);

    state.log_buffers.lock().unwrap().insert(id, Arc::new(Mutex::new(Vec::new())));

    save_apps_to_disk(&state);

    Ok(CommandResult::success(config))
}

#[tauri::command]
pub fn update_app(
    state: State<'_, AppState>,
    params: UpdateAppParams,
) -> Result<CommandResult<AppConfig>, String> {
    let mut apps = state.apps.lock().unwrap();

    if let Some(instance) = apps.get_mut(&params.id) {
        if instance.running {
            return Ok(CommandResult::error("应用正在运行，请先关闭后再编辑"));
        }

        instance.config.name = params.name;
        if let Some(app_type) = params.app_type {
            instance.config.app_type = app_type;
        }
        instance.config.command = params.command;
        instance.config.work_dir = params.work_dir;
        instance.config.description = params.description;
        instance.config.color = params.color;
        instance.config.auto_start = params.auto_start.unwrap_or(false);
        instance.config.group = params.group;
        instance.config.env_vars = params.env_vars;
        instance.config.delay_seconds = params.delay_seconds.unwrap_or(0);
        instance.config.static_server = params.static_server;
        instance.config.url = params.url;
        instance.config.watch_restart = params.watch_restart.unwrap_or(false);
        instance.config.watch_dirs = params.watch_dirs;
        instance.config.exit_restart = params.exit_restart.unwrap_or(false);

        let config = instance.config.clone();
        drop(apps);
        save_apps_to_disk(&state);
        Ok(CommandResult::success(config))
    } else {
        Ok(CommandResult::error("应用不存在"))
    }
}

#[tauri::command]
pub fn delete_app(
    state: State<'_, AppState>,
    id: String,
) -> Result<CommandResult<()>, String> {
    let mut apps = state.apps.lock().unwrap();

    if let Some(instance) = apps.get(&id) {
        if instance.running {
            return Ok(CommandResult::error("应用正在运行，请先关闭后再删除"));
        }
        apps.remove(&id);
        drop(apps);
        state.log_buffers.lock().unwrap().remove(&id);
        save_apps_to_disk(&state);
        Ok(CommandResult {
            code: 0,
            data: None,
            msg: "删除成功".to_string(),
        })
    } else {
        Ok(CommandResult::error("应用不存在"))
    }
}

#[tauri::command]
pub fn list_apps(state: State<'_, AppState>) -> Result<CommandResult<Vec<AppInstance>>, String> {
    let apps = state.apps.lock().unwrap();
    let mut list: Vec<AppInstance> = apps.values().cloned().collect();
    list.sort_by_key(|a| a.config.sort_order);
    Ok(CommandResult::success(list))
}

#[tauri::command]
pub async fn start_app(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<CommandResult<String>, String> {
    // 第一阶段：锁内确认状态并启动/收集数据，锁在块结束时释放
    let (pid, sc_clone): (Option<u32>, Option<StaticServerConfig>) = {
        let mut apps = state.apps.lock().unwrap();
        let Some(instance) = apps.get_mut(&id) else {
            return Ok(CommandResult::error("应用不存在"));
        };

        if instance.running {
            return Ok(CommandResult::error("应用已经在运行中"));
        }

        match instance.config.app_type {
            AppType::Command => {
                if spawn_and_track(instance, "", &state.children, &state.log_buffers, &state.job_object) {
                    (Some(instance.pid.unwrap()), None)
                } else {
                    return Ok(CommandResult::error("启动失败"));
                }
            }
            AppType::StaticServer => match &instance.config.static_server {
                Some(sc) => (None, Some(sc.clone())),
                None => return Ok(CommandResult::error("缺少静态服务器配置")),
            },
        }
    };

    // 锁已释放
    if let Some(pid) = pid {
        save_apps_to_disk(&state);
        start_file_watcher(&state, &app, &id);
        return Ok(CommandResult::success(pid.to_string()));
    }

    let Some(sc) = sc_clone else {
        return Ok(CommandResult::error("缺少静态服务器配置"));
    };
    let port = sc.port;

    match static_server::start_static_server(&sc).await {
        Ok(abort_handle) => {
            state.running_servers.lock().unwrap().insert(id.clone(), abort_handle);
            {
                let mut apps = state.apps.lock().unwrap();
                if let Some(inst) = apps.get_mut(&id) {
                    inst.running = true;
                    inst.server_port = Some(port);
                    inst.started_at = Some(now_ts());
                    push_to_buffer(&state.log_buffers, &id, "info", &format!("静态服务器启动，端口: {}", port));
                }
            }
            save_apps_to_disk(&state);
            start_file_watcher(&state, &app, &id);
            Ok(CommandResult::success(format!("http://localhost:{}", port)))
        }
        Err(e) => {
            let mut apps = state.apps.lock().unwrap();
            if let Some(_) = apps.get_mut(&id) {
                push_to_buffer(&state.log_buffers, &id, "error", &format!("静态服务器启动失败: {}", e));
            }
            Ok(CommandResult::error(format!("启动失败: {}", e)))
        }
    }
}

#[tauri::command]
pub async fn stop_app(
    state: State<'_, AppState>,
    id: String,
    force: bool,
) -> Result<CommandResult<()>, String> {
    // 锁内处理状态变更；Command 分支无 await，在锁内完成并返回
    {
        let mut apps = state.apps.lock().unwrap();
        let Some(instance) = apps.get_mut(&id) else {
            return Ok(CommandResult::error("应用不存在"));
        };
        if !instance.running {
            return Ok(CommandResult::error("应用未在运行中"));
        }

        match instance.config.app_type {
            AppType::StaticServer => {
                push_to_buffer(&state.log_buffers, &id, "info", "正在停止静态服务器...");
                instance.running = false;
                instance.pid = None;
                instance.process_info = None;
                instance.started_at = None;
                instance.server_port = None;
                instance.exit_reason = Some("手动停止".to_string());
                instance.manual_stop = true;
                push_to_buffer(&state.log_buffers, &id, "info", "静态服务器已停止");
            }
            AppType::Command => {
                push_to_buffer(&state.log_buffers, &id, "info", if force { "强制终止进程" } else { "终止进程" });
                let real_pid = instance.pid;
                drop(apps);

                let killed = kill_child(&state.children, &id, force, real_pid);

                let mut apps = state.apps.lock().unwrap();
                if let Some(instance) = apps.get_mut(&id) {
                    if killed || force {
                        instance.running = false;
                        instance.pid = None;
                        instance.process_info = None;
                        instance.started_at = None;
                        instance.exit_reason = Some(if force { "手动强制关闭".to_string() } else { "手动关闭".to_string() });
                        instance.manual_stop = true;
                        push_to_buffer(&state.log_buffers, &id, "info", if force { "已强制关闭" } else { "已关闭" });
                    } else {
                        push_to_buffer(&state.log_buffers, &id, "error", "关闭失败");
                    }
                }
                drop(apps);
                state.children.lock().unwrap().remove(&id);
                stop_file_watcher(&state, &id);
                save_apps_to_disk(&state);

                if killed || force {
                    return Ok(CommandResult {
                        code: 0,
                        data: None,
                        msg: if force { "已强制关闭".to_string() } else { "已关闭".to_string() },
                    });
                }
                return Ok(CommandResult::error("关闭失败"));
            }
        }
    }
    // 锁已释放，继续静态服务器停止流程
    if let Some(handle) = state.running_servers.lock().unwrap().remove(&id) {
        handle.abort();
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    stop_file_watcher(&state, &id);
    save_apps_to_disk(&state);
    Ok(CommandResult {
        code: 0,
        data: None,
        msg: "静态服务器已停止".to_string(),
    })
}

#[tauri::command]
pub async fn restart_app(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<CommandResult<String>, String> {
    let app_type = {
        let apps = state.apps.lock().unwrap();
        match apps.get(&id) {
            Some(instance) => Some(instance.config.app_type.clone()),
            None => None,
        }
    };

    let Some(app_type) = app_type else {
        return Ok(CommandResult::error("应用不存在"));
    };

    {
        let mut apps = state.apps.lock().unwrap();
        if let Some(instance) = apps.get_mut(&id) {
            if instance.running {
                match instance.config.app_type {
                    AppType::Command => {
                        push_to_buffer(&state.log_buffers, &id, "info", "重启：关闭旧进程");
                        instance.running = false;
                        instance.pid = None;
                        instance.process_info = None;
                        instance.started_at = None;
                        instance.exit_reason = Some("重启".to_string());
                    }
                    AppType::StaticServer => {
                        push_to_buffer(&state.log_buffers, &id, "info", "重启：停止静态服务器");
                        instance.running = false;
                        instance.pid = None;
                        instance.process_info = None;
                        instance.started_at = None;
                        instance.server_port = None;
                        instance.exit_reason = Some("重启".to_string());
                    }
                }
            }
        }
    }

    if app_type == AppType::Command {
        kill_child(&state.children, &id, true, None);
        state.children.lock().unwrap().remove(&id);
        stop_file_watcher(&state, &id);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    if app_type == AppType::StaticServer {
        if let Some(handle) = state.running_servers.lock().unwrap().remove(&id) {
            handle.abort();
        }
        stop_file_watcher(&state, &id);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    match app_type {
        AppType::Command => {
            let mut apps = state.apps.lock().unwrap();
            if let Some(instance) = apps.get_mut(&id) {
                if spawn_and_track(instance, "重启", &state.children, &state.log_buffers, &state.job_object) {
                    let pid = instance.pid.unwrap();
                    drop(apps);
                    save_apps_to_disk(&state);
                    start_file_watcher(&state, &app, &id);
                    Ok(CommandResult::success(pid.to_string()))
                } else {
                    Ok(CommandResult::error("重启失败"))
                }
            } else {
                Ok(CommandResult::error("应用不存在"))
            }
        }
        AppType::StaticServer => {
            let sc_clone;
            let port;
            {
                let apps = state.apps.lock().unwrap();
                if let Some(instance) = apps.get(&id) {
                    if let Some(ref sc) = instance.config.static_server {
                        sc_clone = sc.clone();
                        port = sc.port;
                    } else {
                        return Ok(CommandResult::error("缺少静态服务器配置"));
                    }
                } else {
                    return Ok(CommandResult::error("应用不存在"));
                }
            }

            match static_server::start_static_server(&sc_clone).await
            {
                Ok(abort_handle) => {
                    state.running_servers.lock().unwrap().insert(id.clone(), abort_handle);
                    let mut apps = state.apps.lock().unwrap();
                    if let Some(inst) = apps.get_mut(&id) {
                        inst.running = true;
                        inst.server_port = Some(port);
                        inst.started_at = Some(now_ts());
                        push_to_buffer(&state.log_buffers, &id, "info", &format!("静态服务器重启成功，端口: {}", port));
                    }
                    drop(apps);
                    save_apps_to_disk(&state);
                    start_file_watcher(&state, &app, &id);
                    Ok(CommandResult::success(format!("http://localhost:{}", port)))
                }
                Err(e) => Ok(CommandResult::error(format!("重启失败: {}", e))),
            }
        }
    }
}

#[tauri::command]
pub async fn start_all_apps(state: State<'_, AppState>) -> Result<CommandResult<Vec<String>>, String> {
    // 先在锁内收集启动计划，锁在块结束时释放
    let plan: Vec<(String, AppType, u32, Option<StaticServerConfig>)> = {
        let apps = state.apps.lock().unwrap();
        let mut sorted_ids: Vec<String> = apps
            .iter()
            .filter(|(_, i)| !i.running)
            .map(|(id, _)| id.clone())
            .collect();

        sorted_ids.sort_by(|a, b| {
            let a_order = apps.get(a).map(|i| i.config.sort_order).unwrap_or(0);
            let b_order = apps.get(b).map(|i| i.config.sort_order).unwrap_or(0);
            a_order.cmp(&b_order)
        });

        sorted_ids
            .into_iter()
            .filter_map(|id| {
                apps.get(&id).map(|i| {
                    (
                        id,
                        i.config.app_type.clone(),
                        i.config.delay_seconds,
                        i.config.static_server.clone(),
                    )
                })
            })
            .collect()
    };

    let mut started = Vec::new();
    for (id, app_type, delay, sc) in plan {
        if delay > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(delay as u64)).await;
        }

        match app_type {
            AppType::Command => {
                let mut apps = state.apps.lock().unwrap();
                if let Some(instance) = apps.get_mut(&id) {
                    if spawn_and_track(instance, "批量", &state.children, &state.log_buffers, &state.job_object) {
                        started.push(instance.config.name.clone());
                    }
                }
            }
            AppType::StaticServer => {
                if let Some(sc) = sc {
                    let port = sc.port;
                    match static_server::start_static_server(&sc).await {
                        Ok(abort_handle) => {
                            state.running_servers.lock().unwrap().insert(id.clone(), abort_handle);
                            let mut apps = state.apps.lock().unwrap();
                            if let Some(inst) = apps.get_mut(&id) {
                                inst.running = true;
                                inst.server_port = Some(port);
                                inst.started_at = Some(now_ts());
                                push_to_buffer(&state.log_buffers, &id, "info", &format!("批量启动静态服务器，端口: {}", port));
                                started.push(inst.config.name.clone());
                            }
                        }
                        Err(e) => {
                            let mut apps = state.apps.lock().unwrap();
                            if let Some(_) = apps.get_mut(&id) {
                                push_to_buffer(&state.log_buffers, &id, "error", &format!("批量启动静态服务器失败: {}", e));
                            }
                        }
                    }
                }
            }
        }
    }

    save_apps_to_disk(&state);
    Ok(CommandResult::success(started))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostsEntry {
    pub ip: String,
    pub host: String,
    pub enabled: bool,
    pub original_line: String,
    pub line_number: usize,
}

fn get_hosts_path() -> String {
    #[cfg(target_os = "windows")]
    {
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        format!("{}\\System32\\drivers\\etc\\hosts", system_root)
    }
    #[cfg(not(target_os = "windows"))]
    {
        "/etc/hosts".to_string()
    }
}

#[tauri::command]
pub fn tool_read_hosts() -> Result<CommandResult<Vec<HostsEntry>>, String> {
    let path = get_hosts_path();

    if !std::path::Path::new(&path).exists() {
        return Ok(CommandResult::success(vec![]));
    }

    let content = fs::read_to_string(&path).map_err(|e| format!("读取 hosts 文件失败: {}", e))?;

    let mut entries: Vec<HostsEntry> = Vec::new();
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            entries.push(HostsEntry {
                ip: parts[0].to_string(),
                host: parts[1..].join(" "),
                enabled: true,
                original_line: line.to_string(),
                line_number: i + 1,
            });
        }
    }

    Ok(CommandResult::success(entries))
}

#[derive(Debug, Deserialize)]
pub struct WriteHostsParams {
    pub entries: Vec<HostsEntryParams>,
}

#[derive(Debug, Deserialize)]
pub struct HostsEntryParams {
    pub ip: String,
    pub host: String,
    pub enabled: bool,
}

#[tauri::command]
pub fn tool_write_hosts(params: WriteHostsParams) -> Result<CommandResult<String>, String> {
    let path = get_hosts_path();

    let original = if std::path::Path::new(&path).exists() {
        fs::read_to_string(&path).map_err(|e| format!("读取 hosts 文件失败: {}", e))?
    } else {
        String::new()
    };

    if !original.is_empty() {
        let backup_path = format!("{}.app-m.bak", path);
        let _ = fs::write(&backup_path, &original);
    }

    let mut comment_lines: Vec<String> = Vec::new();
    for line in original.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            comment_lines.push(line.to_string());
        }
    }

    let mut new_content = String::new();

    for comment in &comment_lines {
        new_content.push_str(comment);
        new_content.push('\n');
    }

    if !comment_lines.is_empty() {
        new_content.push('\n');
    }

    for entry in &params.entries {
        let line = if entry.enabled {
            format!("{} {}\n", entry.ip, entry.host)
        } else {
            format!("# {} {}\n", entry.ip, entry.host)
        };
        new_content.push_str(&line);
    }

    fs::write(&path, new_content).map_err(|e| format!("写入 hosts 文件失败: {}，可能需要管理员权限运行本程序", e))?;

    Ok(CommandResult::success("hosts 文件已更新".to_string()))
}

#[tauri::command]
pub fn tool_flush_dns() -> Result<CommandResult<String>, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("ipconfig")
            .args(["/flushdns"])
            .creation_flags(0x08000000)
            .output();

        match output {
            Ok(o) => {
                if o.status.success() {
                    Ok(CommandResult::success("DNS 缓存已刷新".to_string()))
                } else {
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    Ok(CommandResult::error(format!("刷新 DNS 失败: {}", stderr)))
                }
            }
            Err(e) => Ok(CommandResult::error(format!("执行 ipconfig 失败: {}", e))),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(CommandResult::error("此功能仅支持 Windows"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessItem {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub memory_mb: f64,
}

#[tauri::command]
pub fn tool_find_process(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<CommandResult<Vec<ProcessItem>>, String> {
    if keyword.is_empty() {
        return Ok(CommandResult::success(vec![]));
    }
    let mut sys = state.system.lock().unwrap();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let kw = keyword.to_lowercase();
    let mut results: Vec<ProcessItem> = sys.processes()
        .iter()
        .filter(|(_, p)| p.name().to_string_lossy().to_lowercase().contains(&kw))
        .map(|(pid, p)| {
            let mem = p.memory();
            ProcessItem {
                pid: pid.as_u32(),
                name: p.name().to_string_lossy().to_string(),
                cpu_usage: (p.cpu_usage() * 100.0).round() / 100.0,
                memory_bytes: mem,
                memory_mb: (mem as f64 / 1024.0 / 1024.0 * 100.0).round() / 100.0,
            }
        })
        .collect();

    results.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap_or(std::cmp::Ordering::Equal));
    Ok(CommandResult::success(results))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub protocol: String,
    pub local_addr: String,
    pub port: u16,
    pub pid: u32,
    pub process_name: String,
    pub state: String,
}

#[tauri::command]
pub fn tool_find_port(
    state: State<'_, AppState>,
    port: u16,
) -> Result<CommandResult<Vec<PortMapping>>, String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("netstat")
            .args(["-ano", "-p", "TCP"])
            .creation_flags(0x08000000)
            .output();

        let output = match output {
            Ok(o) => o,
            Err(e) => return Ok(CommandResult::error(format!("执行 netstat 失败: {}", e))),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut results: Vec<PortMapping> = Vec::new();

        let mut sys = state.system.lock().unwrap();
        sys.refresh_processes(ProcessesToUpdate::All, true);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                continue;
            }

            let local_addr = parts[1];
            if let Some(addr_port) = local_addr.rfind(':') {
                let p: u16 = match local_addr[addr_port + 1..].parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if p != port {
                    continue;
                }

                let pid: u32 = match parts[parts.len() - 1].parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let conn_state = if parts.len() >= 5 { parts[3].to_string() } else { String::new() };

                let process_name = sys.process(Pid::from_u32(pid))
                    .map(|p| p.name().to_string_lossy().to_string())
                    .unwrap_or_else(|| "未知".to_string());

                results.push(PortMapping {
                    protocol: "TCP".to_string(),
                    local_addr: local_addr.to_string(),
                    port: p,
                    pid,
                    process_name,
                    state: conn_state,
                });
            }
        }

        Ok(CommandResult::success(results))
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (state, port);
        Ok(CommandResult::error("此功能仅支持 Windows"))
    }
}

#[tauri::command]
pub fn tool_kill_pid(pid: u32) -> Result<CommandResult<String>, String> {
    let killed = kill_tree(pid);
    if killed {
        Ok(CommandResult::success(format!("进程 {} 已终止", pid)))
    } else {
        Ok(CommandResult::error(format!("终止进程 {} 失败，可能进程不存在或权限不足", pid)))
    }
}

#[tauri::command]
pub fn get_monitoring(state: State<'_, AppState>) -> Result<CommandResult<bool>, String> {
    let enabled = *state.monitoring_enabled.lock().unwrap();
    Ok(CommandResult::success(enabled))
}

#[tauri::command]
pub fn toggle_monitoring(state: State<'_, AppState>, enabled: bool) -> Result<CommandResult<bool>, String> {
    *state.monitoring_enabled.lock().unwrap() = enabled;
    if !enabled {
        state.metrics.lock().unwrap().clear();
        state.net_io_prev.lock().unwrap().clear();
    }
    Ok(CommandResult::success(enabled))
}

#[tauri::command]
pub fn get_metrics(
    state: State<'_, AppState>,
    id: String,
) -> Result<CommandResult<Vec<MetricPoint>>, String> {
    let metrics = state.metrics.lock().unwrap();
    let data = metrics.get(&id).cloned().unwrap_or_default();
    Ok(CommandResult::success(data))
}

#[tauri::command]
pub async fn stop_all_apps(state: State<'_, AppState>) -> Result<CommandResult<Vec<String>>, String> {
    let ids: Vec<String> = {
        let apps = state.apps.lock().unwrap();
        apps.iter()
            .filter(|(_, i)| i.running)
            .map(|(id, _)| id.clone())
            .collect()
    };

    let mut stopped = Vec::new();
    for id in &ids {
        let _ = kill_child(&state.children, id, true, None);
    }

    {
        let mut children = state.children.lock().unwrap();
        for id in &ids {
            children.remove(id);
        }
    }

    let server_ids: Vec<String> = {
        let apps = state.apps.lock().unwrap();
        apps.iter()
            .filter(|(_id, i)| i.running && matches!(i.config.app_type, AppType::StaticServer))
            .map(|(id, _)| id.clone())
            .collect()
    };

    for id in &server_ids {
        if let Some(handle) = state.running_servers.lock().unwrap().remove(id) {
            handle.abort();
        }
    }

    if !server_ids.is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    let mut apps = state.apps.lock().unwrap();
    for id in &ids {
        if let Some(instance) = apps.get_mut(id) {
            instance.running = false;
            instance.pid = None;
            instance.process_info = None;
            instance.started_at = None;
            instance.server_port = None;
            instance.exit_reason = Some("批量关闭".to_string());
            instance.manual_stop = true;
            push_to_buffer(&state.log_buffers, id, "info", "批量关闭");
            stopped.push(instance.config.name.clone());
        }
    }

    drop(apps);
    for id in &ids {
        stop_file_watcher(&state, id);
    }
    save_apps_to_disk(&state);
    Ok(CommandResult::success(stopped))
}

#[tauri::command]
pub fn get_process_info(
    state: State<'_, AppState>,
    id: String,
) -> Result<CommandResult<Option<ProcessInfo>>, String> {
    let apps = state.apps.lock().unwrap();
    if let Some(instance) = apps.get(&id) {
        Ok(CommandResult::success(instance.process_info.clone()))
    } else {
        Ok(CommandResult::error("应用不存在"))
    }
}

#[tauri::command]
pub fn get_app_logs(
    state: State<'_, AppState>,
    id: String,
) -> Result<CommandResult<Vec<LogEntry>>, String> {
    let buffers = state.log_buffers.lock().unwrap();
    if let Some(buf) = buffers.get(&id) {
        let logs = buf.lock().unwrap();
        return Ok(CommandResult::success(logs.clone()));
    }
    Ok(CommandResult::success(vec![]))
}

#[tauri::command]
pub fn clear_app_logs(
    state: State<'_, AppState>,
    id: String,
) -> Result<CommandResult<()>, String> {
    let buffers = state.log_buffers.lock().unwrap();
    if let Some(buf) = buffers.get(&id) {
        buf.lock().unwrap().clear();
    }
    Ok(CommandResult { code: 0, data: None, msg: "日志已清空".to_string() })
}

#[cfg(target_os = "windows")]
fn read_process_io(pid: u32) -> (u64, u64) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetProcessIoCounters, OpenProcess, IO_COUNTERS, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return (0, 0);
        }
        let mut counters = IO_COUNTERS::default();
        let ok = GetProcessIoCounters(handle, &mut counters);
        CloseHandle(handle);
        if ok != 0 {
            (counters.ReadTransferCount, counters.WriteTransferCount)
        } else {
            (0, 0)
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn read_process_io(_pid: u32) -> (u64, u64) {
    (0, 0)
}

#[tauri::command]
pub async fn refresh_all(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<AppInstance>>, String> {
    let pids: Vec<Pid> = {
        let apps = state.apps.lock().unwrap();
        apps.values().filter_map(|i| i.pid).map(Pid::from_u32).collect()
    };
    let (restart_ids, has_changes) = {
        let mut sys = state.system.lock().unwrap();
        sys.refresh_processes(ProcessesToUpdate::Some(&pids), true);

        let mut apps = state.apps.lock().unwrap();
        let mut children = state.children.lock().unwrap();
        let mut exited_apps: Vec<String> = Vec::new();

        let app_ids: Vec<String> = apps.keys().cloned().collect();

        for id in &app_ids {
            if let Some(instance) = apps.get_mut(id) {
                if let Some(pid) = instance.pid {
                    let pid_obj = Pid::from_u32(pid);
                    if let Some(process) = sys.process(pid_obj) {
                        let cpu = process.cpu_usage();
                        let mem = process.memory();
                        instance.process_info = Some(ProcessInfo {
                            pid,
                            cpu_usage: (cpu * 100.0).round() / 100.0,
                            memory_bytes: mem,
                            memory_mb: (mem as f64 / 1024.0 / 1024.0 * 100.0).round() / 100.0,
                            status: if cpu > 0.0 { "running".to_string() } else { "sleeping".to_string() },
                        });
                        instance.running = true;

                        let monitoring = *state.monitoring_enabled.lock().unwrap();
                        if monitoring {
                            let ts = now_ts();
                            let (io_in, io_out) = read_process_io(pid);
                            let (net_in, net_out) = {
                                let mut prev = state.net_io_prev.lock().unwrap();
                                let rates = match prev.get(id) {
                                    Some(&(prev_ts, prev_in, prev_out)) => {
                                        let dt = (ts - prev_ts).max(1) as f32;
                                        (
                                            (io_in.saturating_sub(prev_in)) as f32 / dt,
                                            (io_out.saturating_sub(prev_out)) as f32 / dt,
                                        )
                                    }
                                    None => (0.0, 0.0),
                                };
                                prev.insert(id.clone(), (ts, io_in, io_out));
                                rates
                            };

                            let mut metrics = state.metrics.lock().unwrap();
                            let history = metrics.entry(id.clone()).or_insert_with(Vec::new);
                            history.push(MetricPoint {
                                ts,
                                cpu: (cpu * 100.0).round() / 100.0,
                                mem,
                                net_in: (net_in * 100.0).round() / 100.0,
                                net_out: (net_out * 100.0).round() / 100.0,
                            });
                            let one_hour_ago = ts - 3600;
                            while history.first().map(|p| p.ts) < Some(one_hour_ago) {
                                history.remove(0);
                            }
                        }
                    } else {
                        let mut exit_msg = "进程已退出".to_string();
                        let should_auto_restart = instance.config.exit_restart
                            && matches!(instance.config.app_type, AppType::Command);

                        if let Some(child) = children.get_mut(id) {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    if status.success() {
                                        exit_msg = format!("进程正常退出 (退出码: 0)");
                                    } else if let Some(code) = status.code() {
                                        exit_msg = format!("进程异常退出 (退出码: {})", code);
                                    } else {
                                        exit_msg = "进程被信号终止".to_string();
                                    }
                                }
                                Ok(None) => {
                                    exit_msg = "进程已退出 (系统无法获取状态)".to_string();
                                }
                                Err(e) => {
                                    exit_msg = format!("进程已退出 (获取状态失败: {})", e);
                                }
                            }
                        }

                        if instance.running {
                            push_to_buffer(&state.log_buffers, id, "warn", &exit_msg);
                            exited_apps.push(instance.config.name.clone());
                        }
                        instance.running = false;
                        instance.pid = None;
                        instance.process_info = None;
                        instance.started_at = None;
                        instance.exit_reason = if should_auto_restart {
                            Some(format!("{} (自动重启中...)", exit_msg))
                        } else {
                            Some(exit_msg)
                        };
                    }
                }
            }
        }

        let mut restart_ids: Vec<String> = Vec::new();
        // 直接复用上方已持有的 apps 锁，避免对同一 Mutex 重复加锁导致自死锁
        for id in &app_ids {
            if let Some(inst) = apps.get(id) {
                if !inst.running && inst.config.exit_restart && inst.pid.is_none() && !inst.manual_stop {
                    if matches!(inst.config.app_type, AppType::Command) {
                        restart_ids.push(id.clone());
                    }
                }
            }
        }

        children.retain(|_id, child| {
            match child.try_wait() {
                Ok(None) => true,
                _ => false,
            }
        });

        let has_changes = !exited_apps.is_empty() || !restart_ids.is_empty();
        (restart_ids, has_changes)
    };
    // 锁已全部释放

    if has_changes {
        save_apps_to_disk(&state);
    }

    if !restart_ids.is_empty() {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let mut apps = state.apps.lock().unwrap();
        for rid in &restart_ids {
            if let Some(instance) = apps.get_mut(rid) {
                if !instance.running {
                    push_to_buffer(&state.log_buffers, rid, "info", "退出自动重启：重新启动进程...");
                    if spawn_and_track(instance, "自动重启", &state.children, &state.log_buffers, &state.job_object) {
                        push_to_buffer(&state.log_buffers, rid, "info", "退出自动重启成功");
                    } else {
                        push_to_buffer(&state.log_buffers, rid, "error", "退出自动重启失败");
                    }
                }
            }
        }
        drop(apps);
        save_apps_to_disk(&state);
    }

    let apps = state.apps.lock().unwrap();
    let mut list: Vec<AppInstance> = apps.values().cloned().collect();
    list.sort_by_key(|a| a.config.sort_order);
    Ok(CommandResult::success(list))
}

#[tauri::command]
pub fn get_system_info(state: State<'_, AppState>) -> Result<CommandResult<SystemInfo>, String> {
    let mut sys = state.system.lock().unwrap();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_usage = sys.global_cpu_usage();
    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();

    let total_memory_gb = (total_memory as f64 / 1024.0 / 1024.0 / 1024.0 * 100.0).round() / 100.0;
    let used_memory_gb = (used_memory as f64 / 1024.0 / 1024.0 / 1024.0 * 100.0).round() / 100.0;
    let memory_usage_percent = if total_memory > 0 {
        (used_memory as f64 / total_memory as f64 * 100.0 * 100.0).round() / 100.0
    } else {
        0.0
    };

    Ok(CommandResult::success(SystemInfo {
        cpu_usage: (cpu_usage * 100.0).round() / 100.0,
        total_memory_gb,
        used_memory_gb,
        memory_usage_percent,
    }))
}

#[tauri::command]
pub fn load_apps(state: State<'_, AppState>) -> Result<CommandResult<Vec<AppInstance>>, String> {
    let path = get_data_file_path(&state);
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(data) = serde_json::from_str::<PersistData>(&content) {
            let max_order = data.apps.iter().map(|i| i.config.sort_order).max().unwrap_or(-1);
            state.init_sort_order(max_order);

            let pids: Vec<Pid> = data
                .apps
                .iter()
                .filter(|i| i.running && i.pid.is_some())
                .filter_map(|i| i.pid)
                .map(Pid::from_u32)
                .collect();
            state
                .system
                .lock()
                .unwrap()
                .refresh_processes(ProcessesToUpdate::Some(&pids), true);

            // 存活预检：先单独持 system 锁（与 refresh_all 的 system -> apps 顺序一致），
            // 避免在持有 apps 锁时再取 system 锁造成跨线程死锁
            let mut alive_map: HashMap<String, Option<bool>> = HashMap::new();
            {
                let sys = state.system.lock().unwrap();
                for instance in &data.apps {
                    if !instance.running {
                        continue;
                    }
                    let alive = match instance.config.app_type {
                        AppType::Command => {
                            instance.pid.map(|pid| sys.process(Pid::from_u32(pid)).is_some())
                        }
                        AppType::StaticServer => Some(false),
                    };
                    alive_map.insert(instance.config.id.clone(), alive);
                }
            }

            let mut apps = state.apps.lock().unwrap();
            let mut buffers = state.log_buffers.lock().unwrap();
            for mut instance in data.apps {
                let id = instance.config.id.clone();
                instance.logs = None;

                if instance.running {
                    match instance.config.app_type {
                        AppType::Command => {
                            if instance.pid.is_some()
                                && alive_map.get(&id).copied().flatten() == Some(false)
                            {
                                instance.running = false;
                                instance.pid = None;
                                instance.started_at = None;
                                instance.exit_reason = Some("进程已退出".to_string());
                            } else if instance.pid.is_none() {
                                instance.running = false;
                                instance.exit_reason = Some("进程信息丢失".to_string());
                            }
                        }
                        AppType::StaticServer => {
                            instance.running = false;
                            instance.server_port = None;
                            instance.exit_reason = Some("静态服务器需要重新启动".to_string());
                        }
                    }
                }

                apps.insert(id.clone(), instance);
                buffers.insert(id, Arc::new(Mutex::new(Vec::new())));
            }

            let mut list: Vec<AppInstance> = apps.values().cloned().collect();
            list.sort_by_key(|a| a.config.sort_order);
            return Ok(CommandResult::success(list));
        }
    }
    Ok(CommandResult::success(vec![]))
}

#[tauri::command]
pub fn export_config(state: State<'_, AppState>) -> Result<CommandResult<String>, String> {
    let apps = state.apps.lock().unwrap();
    let mut instances: Vec<AppInstance> = apps.values().map(|i| {
        let mut inst = i.clone();
        inst.running = false;
        inst.pid = None;
        inst.process_info = None;
        inst.started_at = None;
        inst.logs = None;
        inst.server_port = None;
        inst.exit_reason = None;
        inst
    }).collect();
    instances.sort_by_key(|a| a.config.sort_order);
    let data = PersistData { apps: instances };
    match serde_json::to_string_pretty(&data) {
        Ok(json) => Ok(CommandResult::success(json)),
        Err(e) => Ok(CommandResult::error(format!("导出失败: {}", e))),
    }
}

#[tauri::command]
pub fn import_config(
    state: State<'_, AppState>,
    json: String,
) -> Result<CommandResult<usize>, String> {
    match serde_json::from_str::<PersistData>(&json) {
        Ok(data) => {
            let mut count = 0;
            let mut apps = state.apps.lock().unwrap();
            let mut buffers = state.log_buffers.lock().unwrap();
            for mut instance in data.apps {
                if !apps.contains_key(&instance.config.id) {
                    let id = instance.config.id.clone();
                    let sort_order = state.alloc_sort_order();
                    instance.config.sort_order = sort_order;
                    instance.running = false;
                    instance.pid = None;
                    instance.process_info = None;
                    instance.started_at = None;
                    instance.logs = None;
                    instance.server_port = None;
                    instance.exit_reason = None;

                    apps.insert(id.clone(), instance);
                    buffers.insert(id, Arc::new(Mutex::new(Vec::new())));
                    count += 1;
                }
            }
            drop(apps);
            drop(buffers);
            save_apps_to_disk(&state);
            Ok(CommandResult::success(count))
        }
        Err(e) => Ok(CommandResult::error(format!("解析失败: {}", e))),
    }
}

#[tauri::command]
pub fn update_sort_order(
    state: State<'_, AppState>,
    params: UpdateSortOrderParams,
) -> Result<CommandResult<()>, String> {
    let mut apps = state.apps.lock().unwrap();
    for (id, order) in params.orders {
        if let Some(instance) = apps.get_mut(&id) {
            instance.config.sort_order = order;
        }
    }
    drop(apps);
    save_apps_to_disk(&state);
    Ok(CommandResult {
        code: 0,
        data: None,
        msg: "排序已更新".to_string(),
    })
}

#[tauri::command]
pub fn get_groups(state: State<'_, AppState>) -> Result<CommandResult<Vec<String>>, String> {
    let apps = state.apps.lock().unwrap();
    let mut groups: Vec<String> = apps
        .values()
        .filter_map(|i| i.config.group.clone())
        .collect();
    groups.sort();
    groups.dedup();
    Ok(CommandResult::success(groups))
}

#[tauri::command]
pub async fn start_auto_start_apps(
    state: State<'_, AppState>,
) -> Result<CommandResult<Vec<String>>, String> {
    // 先在锁内收集启动计划，锁在块结束时释放
    let plan: Vec<(String, u32)> = {
        let apps = state.apps.lock().unwrap();
        let mut sorted_ids: Vec<String> = apps
            .iter()
            .filter(|(_, i)| i.config.auto_start && !i.running)
            .map(|(id, _)| id.clone())
            .collect();

        sorted_ids.sort_by(|a, b| {
            let a_order = apps.get(a).map(|i| i.config.sort_order).unwrap_or(0);
            let b_order = apps.get(b).map(|i| i.config.sort_order).unwrap_or(0);
            a_order.cmp(&b_order)
        });

        sorted_ids
            .into_iter()
            .filter_map(|id| apps.get(&id).map(|i| (id, i.config.delay_seconds)))
            .collect()
    };

    let mut started = Vec::new();
    for (id, delay) in plan {
        if delay > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(delay as u64)).await;
        }

        let mut apps = state.apps.lock().unwrap();
        if let Some(instance) = apps.get_mut(&id) {
            if spawn_and_track(instance, "自动", &state.children, &state.log_buffers, &state.job_object) {
                started.push(instance.config.name.clone());
            }
        }
    }

    save_apps_to_disk(&state);
    Ok(CommandResult::success(started))
}
