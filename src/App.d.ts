export type AppType = 'Command' | 'StaticServer'

export interface ProxyRule {
  path: string
  target: string
  rewrite: boolean
}

export interface StaticServerConfig {
  port: number
  root_dir: string
  spa_mode: boolean
  index_file: string
  proxy_rules: ProxyRule[]
}

export interface AppConfig {
  id: string
  name: string
  app_type: AppType
  command: string
  work_dir: string | null
  description: string | null
  color: string | null
  auto_start: boolean
  group: string | null
  env_vars: Record<string, string> | null
  sort_order: number
  delay_seconds: number
  static_server: StaticServerConfig | null
  url: string | null
  watch_restart: boolean
  watch_dirs: string[] | null
  exit_restart: boolean
}

export interface ProcessInfo {
  pid: number
  cpu_usage: number
  memory_bytes: number
  memory_mb: number
  status: string
}

export interface LogEntry {
  timestamp: number
  level: string
  content: string
}

export interface AppInstance {
  config: AppConfig
  pid: number | null
  running: boolean
  process_info: ProcessInfo | null
  started_at: number | null
  logs: LogEntry[] | null
  server_port: number | null
  exit_reason: string | null
  /** 已发送停止信号、等待进程退出 */
  stopping: boolean
}

export interface CommandResult<T> {
  code: number
  data: T | null
  msg: string
}

export interface AddAppParams {
  name: string
  app_type?: AppType | null
  command: string
  work_dir?: string | null
  description?: string | null
  color?: string | null
  auto_start?: boolean | null
  group?: string | null
  env_vars?: Record<string, string> | null
  delay_seconds?: number | null
  static_server?: StaticServerConfig | null
  url?: string | null
  watch_restart?: boolean | null
  watch_dirs?: string[] | null
}

export interface UpdateAppParams {
  id: string
  name: string
  app_type?: AppType | null
  command: string
  work_dir?: string | null
  description?: string | null
  color?: string | null
  auto_start?: boolean | null
  group?: string | null
  env_vars?: Record<string, string> | null
  delay_seconds?: number | null
  static_server?: StaticServerConfig | null
  url?: string | null
  watch_restart?: boolean | null
  watch_dirs?: string[] | null
}

export interface SystemInfo {
  cpu_usage: number
  total_memory_gb: number
  used_memory_gb: number
  memory_usage_percent: number
}

export interface ProcessItem {
  pid: number
  name: string
  cpu_usage: number
  memory_bytes: number
  memory_mb: number
}

export interface PortMapping {
  protocol: string
  local_addr: string
  port: number
  pid: number
  process_name: string
  state: string
}

export interface MetricPoint {
  ts: number
  cpu: number
  mem: number
  net_in: number
  net_out: number
}

export interface HostsEntry {
  ip: string
  host: string
  enabled: boolean
  original_line: string
  line_number: number
}

export class indexTypes {
  static AppConfig: AppConfig = {} as AppConfig
  static ProcessInfo: ProcessInfo = {} as ProcessInfo
  static LogEntry: LogEntry = {} as LogEntry
  static AppInstance: AppInstance = {} as AppInstance
  static CommandResult: CommandResult<unknown> = {} as CommandResult<unknown>
  static AddAppParams: AddAppParams = {} as AddAppParams
  static UpdateAppParams: UpdateAppParams = {} as UpdateAppParams
  static SystemInfo: SystemInfo = {} as SystemInfo
  static AppType: AppType = 'Command' as AppType
  static ProxyRule: ProxyRule = {} as ProxyRule
  static StaticServerConfig: StaticServerConfig = {} as StaticServerConfig
  static ProcessItem: ProcessItem = {} as ProcessItem
  static PortMapping: PortMapping = {} as PortMapping
  static MetricPoint: MetricPoint = {} as MetricPoint
  static HostsEntry: HostsEntry = {} as HostsEntry
}
