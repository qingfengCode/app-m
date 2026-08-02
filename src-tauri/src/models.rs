use crate::job_object::JobObject;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Child;
use std::sync::{Arc, Mutex};
use sysinfo::System;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    pub ts: i64,
    pub cpu: f32,
    pub mem: u64,
    /// 网络下行速率 (bytes/s)
    pub net_in: f32,
    /// 网络上行速率 (bytes/s)
    pub net_out: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub id: String,
    pub name: String,
    pub app_type: AppType,
    pub command: String,
    pub work_dir: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub auto_start: bool,
    pub group: Option<String>,
    pub env_vars: Option<HashMap<String, String>>,
    pub sort_order: i32,
    pub delay_seconds: u32,
    pub static_server: Option<StaticServerConfig>,
    pub url: Option<String>,
    pub watch_restart: bool,
    pub watch_dirs: Option<Vec<String>>,
    pub exit_restart: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AppType {
    Command,
    StaticServer,
}

impl Default for AppType {
    fn default() -> Self {
        AppType::Command
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticServerConfig {
    pub port: u16,
    pub root_dir: String,
    pub spa_mode: bool,
    pub index_file: String,
    pub proxy_rules: Vec<ProxyRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRule {
    pub path: String,
    pub target: String,
    pub rewrite: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub memory_mb: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInstance {
    pub config: AppConfig,
    pub pid: Option<u32>,
    pub running: bool,
    pub process_info: Option<ProcessInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<Vec<LogEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_reason: Option<String>,
    #[serde(skip)]
    pub manual_stop: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: i64,
    pub level: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub cpu_usage: f32,
    pub total_memory_gb: f64,
    pub used_memory_gb: f64,
    pub memory_usage_percent: f64,
}

pub struct AppState {
    pub apps: Mutex<HashMap<String, AppInstance>>,
    pub system: Mutex<System>,
    pub app_data_dir: Mutex<String>,
    pub next_sort_order: Mutex<i32>,
    pub running_servers: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    pub children: Mutex<HashMap<String, Child>>,
    pub log_buffers: Mutex<HashMap<String, Arc<Mutex<Vec<LogEntry>>>>>,
    pub monitoring_enabled: Mutex<bool>,
    pub metrics: Mutex<HashMap<String, Vec<MetricPoint>>>,
    /// 每个应用上一次采集的 (时间戳, 读字节数, 写字节数)，用于计算网络速率
    pub net_io_prev: Mutex<HashMap<String, (i64, u64, u64)>>,
    pub file_watchers: Mutex<HashMap<String, notify::RecommendedWatcher>>,
    pub job_object: JobObject,
}

impl AppState {
    pub fn new(data_dir: String) -> Self {
        let job_object = JobObject::new()
            .expect("Failed to create Job Object");
        Self {
            apps: Mutex::new(HashMap::new()),
            system: Mutex::new(System::new()),
            app_data_dir: Mutex::new(data_dir),
            next_sort_order: Mutex::new(0),
            running_servers: Mutex::new(HashMap::new()),
            children: Mutex::new(HashMap::new()),
            log_buffers: Mutex::new(HashMap::new()),
            monitoring_enabled: Mutex::new(true),
            metrics: Mutex::new(HashMap::new()),
            net_io_prev: Mutex::new(HashMap::new()),
            file_watchers: Mutex::new(HashMap::new()),
            job_object,
        }
    }

    pub fn alloc_sort_order(&self) -> i32 {
        let mut next = self.next_sort_order.lock().unwrap();
        let val = *next;
        *next += 1;
        val
    }

    pub fn init_sort_order(&self, max_order: i32) {
        let mut next = self.next_sort_order.lock().unwrap();
        *next = max_order + 1;
    }

    pub fn cleanup(&self) {
        let ids: Vec<String> = {
            let apps = self.apps.lock().unwrap();
            apps.iter()
                .filter(|(_, i)| i.running)
                .map(|(id, _)| id.clone())
                .collect()
        };

        for id in &ids {
            if let Some(child) = self.children.lock().unwrap().get_mut(id) {
                let pid = child.id();
                let _ = child.kill();
                crate::commands::kill_tree(pid);
            }
        }
        self.children.lock().unwrap().clear();

        let server_handles: Vec<tokio::task::AbortHandle> = {
            let mut servers = self.running_servers.lock().unwrap();
            servers.drain().map(|(_, v)| v).collect()
        };
        for handle in server_handles {
            handle.abort();
        }
        self.file_watchers.lock().unwrap().clear();
    }
}

#[derive(Debug, Deserialize)]
pub struct AddAppParams {
    pub name: String,
    pub app_type: Option<AppType>,
    pub command: String,
    pub work_dir: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub auto_start: Option<bool>,
    pub group: Option<String>,
    pub env_vars: Option<HashMap<String, String>>,
    pub delay_seconds: Option<u32>,
    pub static_server: Option<StaticServerConfig>,
    pub url: Option<String>,
    pub watch_restart: Option<bool>,
    pub watch_dirs: Option<Vec<String>>,
    pub exit_restart: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAppParams {
    pub id: String,
    pub name: String,
    pub app_type: Option<AppType>,
    pub command: String,
    pub work_dir: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub auto_start: Option<bool>,
    pub group: Option<String>,
    pub env_vars: Option<HashMap<String, String>>,
    pub delay_seconds: Option<u32>,
    pub static_server: Option<StaticServerConfig>,
    pub url: Option<String>,
    pub watch_restart: Option<bool>,
    pub watch_dirs: Option<Vec<String>>,
    pub exit_restart: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSortOrderParams {
    pub orders: Vec<(String, i32)>,
}

#[derive(Debug, Serialize)]
pub struct CommandResult<T: Serialize> {
    pub code: i32,
    pub data: Option<T>,
    pub msg: String,
}

impl<T: Serialize> CommandResult<T> {
    pub fn success(data: T) -> Self {
        Self {
            code: 0,
            data: Some(data),
            msg: "success".to_string(),
        }
    }

    pub fn error(msg: impl Into<String>) -> CommandResult<T> {
        CommandResult::<T> {
            code: -1,
            data: None,
            msg: msg.into(),
        }
    }
}

pub fn create_app_config(params: AddAppParams, sort_order: i32) -> AppConfig {
    AppConfig {
        id: Uuid::new_v4().to_string(),
        name: params.name,
        app_type: params.app_type.unwrap_or(AppType::Command),
        command: params.command,
        work_dir: params.work_dir,
        description: params.description,
        color: params.color,
        auto_start: params.auto_start.unwrap_or(false),
        group: params.group,
        env_vars: params.env_vars,
        sort_order,
        delay_seconds: params.delay_seconds.unwrap_or(0),
        static_server: params.static_server,
        url: params.url,
        watch_restart: params.watch_restart.unwrap_or(false),
        watch_dirs: params.watch_dirs,
        exit_restart: params.exit_restart.unwrap_or(false),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PersistData {
    pub apps: Vec<AppInstance>,
}
