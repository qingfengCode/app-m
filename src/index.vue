<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { ElMessage, ElMessageBox, ElNotification } from 'element-plus'
import { Refresh, Plus, VideoPlay, SwitchButton, Search, Download, Upload, Sunny, Moon, Monitor, Grid, List, SetUp } from '@element-plus/icons-vue'
import AppCard from './components/AppCard.vue'
import AppListItem from './components/AppListItem.vue'
import AppFormDialog from './components/AppFormDialog.vue'
import ToolKit from './components/ToolKit.vue'
import type { AppInstance, AppType, CommandResult, SystemInfo } from './App'

const apps = ref<AppInstance[]>([])
const loading = ref(false)
const dialogVisible = ref(false)
const editingApp = ref<AppInstance | null>(null)
const copySource = ref<AppInstance | null>(null)
const toolKitVisible = ref(false)
const searchText = ref('')
const statusFilter = ref<'all' | 'running' | 'stopped'>('all')
const groupFilter = ref('all')
const typeFilter = ref<'all' | AppType>('all')
const groups = ref<string[]>([])
const systemInfo = ref<SystemInfo | null>(null)
const isDark = ref(false)
const lastRefreshTime = ref('')
const viewMode = ref<'grid' | 'list'>('list')
let refreshTimer: ReturnType<typeof setInterval> | null = null

function toggleView() {
  viewMode.value = viewMode.value === 'list' ? 'grid' : 'list'
  localStorage.setItem('app-manager-view', viewMode.value)
}

const filteredApps = computed(() => {
  let list = apps.value
  if (searchText.value) {
    const keyword = searchText.value.toLowerCase()
    list = list.filter(
      (a) =>
        a.config.name.toLowerCase().includes(keyword) ||
        a.config.command.toLowerCase().includes(keyword) ||
        (a.config.group && a.config.group.toLowerCase().includes(keyword)) ||
        (a.config.description && a.config.description.toLowerCase().includes(keyword))
    )
  }
  if (statusFilter.value === 'running') {
    list = list.filter((a) => a.running)
  } else if (statusFilter.value === 'stopped') {
    list = list.filter((a) => !a.running)
  }
  if (groupFilter.value !== 'all' && groupFilter.value !== '') {
    list = list.filter((a) => a.config.group === groupFilter.value)
  }
  if (typeFilter.value !== 'all') {
    list = list.filter((a) => a.config.app_type === typeFilter.value)
  }
  return list
})

const runningCount = computed(() => apps.value.filter((a) => a.running).length)
const stoppedCount = computed(() => apps.value.filter((a) => !a.running).length)

function toggleTheme() {
  isDark.value = !isDark.value
  document.documentElement.classList.toggle('dark', isDark.value)
  localStorage.setItem('app-manager-theme', isDark.value ? 'dark' : 'light')
}

function initTheme() {
  const saved = localStorage.getItem('app-manager-theme')
  if (saved === 'dark') {
    isDark.value = true
    document.documentElement.classList.add('dark')
  }
  const savedView = localStorage.getItem('app-manager-view')
  if (savedView === 'grid' || savedView === 'list') {
    viewMode.value = savedView
  }
}

async function fetchGroups() {
  try {
    const res = await invoke<CommandResult<string[]>>('get_groups')
    if (res.code === 0 && res.data) {
      groups.value = res.data
    }
  } catch (e) {
    console.error('获取分组失败', e)
  }
}

async function loadFromDisk() {
  try {
    const res = await invoke<CommandResult<AppInstance[]>>('load_apps')
    if (res.code === 0 && res.data) {
      apps.value = res.data
    }
  } catch (e) {
    console.error('加载数据失败', e)
  }
}

async function fetchSystemInfo() {
  try {
    const res = await invoke<CommandResult<SystemInfo>>('get_system_info')
    if (res.code === 0 && res.data) {
      systemInfo.value = res.data
    }
  } catch (e) {
    console.error('获取系统信息失败', e)
  }
}

async function fetchApps() {
  try {
    const res = await invoke<CommandResult<AppInstance[]>>('list_apps')
    if (res.code === 0 && res.data) {
      apps.value = res.data
    }
  } catch (e) {
    console.error('获取应用列表失败', e)
  }
}

function updateRefreshTime() {
  const now = new Date()
  lastRefreshTime.value = now.toLocaleTimeString('zh-CN', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  })
}

async function refreshApps() {
  loading.value = true
  try {
    const res = await invoke<CommandResult<AppInstance[]>>('refresh_all')
    if (res.code === 0 && res.data) {
      const prevRunning = new Set(apps.value.filter(a => a.running).map(a => a.config.id))
      apps.value = res.data
      const nowRunning = new Set(apps.value.filter(a => a.running).map(a => a.config.id))
      const exited = apps.value.filter(a => prevRunning.has(a.config.id) && !nowRunning.has(a.config.id))
      if (exited.length > 0) {
        const details = exited.map(a => {
          const reason = a.exit_reason ? ` (${a.exit_reason})` : ''
          return `${a.config.name}${reason}`
        }).join('\n')
        ElNotification({
          title: '进程退出通知',
          message: details,
          type: 'warning',
          duration: 8000
        })
      }
    }
    updateRefreshTime()
  } catch (e) {
    console.error('刷新失败', e)
  } finally {
    loading.value = false
  }
}

// ---- 拖拽排序 ----
const dragId = ref<string | null>(null)
const dropId = ref<string | null>(null)

function handleDragStart(id: string) {
  dragId.value = id
}

function handleDragEnd() {
  dragId.value = null
  dropId.value = null
}

function handleDragEnter(id: string) {
  if (dragId.value && dragId.value !== id) {
    dropId.value = id
  }
}

async function handleReorder(targetId: string) {
  const dragged = dragId.value
  dragId.value = null
  dropId.value = null
  if (!dragged || dragged === targetId) return
  const list = [...apps.value]
  const fromIdx = list.findIndex(a => a.config.id === dragged)
  const toIdx = list.findIndex(a => a.config.id === targetId)
  if (fromIdx < 0 || toIdx < 0) return
  const [moved] = list.splice(fromIdx, 1)
  const insertAt = fromIdx < toIdx ? toIdx - 1 : toIdx
  list.splice(insertAt, 0, moved)
  apps.value = list
  const orders: [string, number][] = list.map((a, i) => [a.config.id, i])
  try {
    await invoke<CommandResult<null>>('update_sort_order', { params: { orders } })
  } catch {
    ElMessage.error('保存排序失败')
    await refreshApps()
  }
}
// ---- 拖拽排序结束 ----

async function handleStart(id: string) {
  try {
    const res = await invoke<CommandResult<string>>('start_app', { id })
    if (res.code === 0) {
      ElMessage.success(`启动成功: ${res.data}`)
      await refreshApps()
    } else {
      ElMessage.error(res.msg)
    }
  } catch (e) {
    ElMessage.error('启动失败')
  }
}

async function handleStop(id: string, force: boolean = false) {
  try {
    const res = await invoke<CommandResult<null>>('stop_app', { id, force })
    if (res.code === 0) {
      ElMessage.success(res.msg)
      await refreshApps()
    } else {
      ElMessage.error(res.msg)
    }
  } catch (e) {
    ElMessage.error('关闭失败')
  }
}

async function handleRestart(id: string) {
  try {
    const res = await invoke<CommandResult<string>>('restart_app', { id })
    if (res.code === 0) {
      ElMessage.success(`重启成功: ${res.data}`)
      await refreshApps()
    } else {
      ElMessage.error(res.msg)
    }
  } catch (e) {
    ElMessage.error('重启失败')
  }
}

async function handleDelete(app: AppInstance) {
  try {
    await ElMessageBox.confirm(
      `确定要删除应用「${app.config.name}」吗？`,
      '确认删除',
      { confirmButtonText: '确定', cancelButtonText: '取消', type: 'warning' }
    )
    const res = await invoke<CommandResult<null>>('delete_app', { id: app.config.id })
    if (res.code === 0) {
      ElMessage.success('删除成功')
      await fetchApps()
      await fetchGroups()
    } else {
      ElMessage.error(res.msg)
    }
  } catch {
    // cancelled
  }
}

async function handleStartAll() {
  try {
    const res = await invoke<CommandResult<string[]>>('start_all_apps')
    if (res.code === 0 && res.data) {
      ElMessage.success(`已启动 ${res.data.length} 个应用`)
      await refreshApps()
    }
  } catch (e) {
    ElMessage.error('批量启动失败')
  }
}

async function handleStopAll() {
  try {
    await ElMessageBox.confirm('确定要关闭所有运行中的应用吗？', '批量关闭', {
      confirmButtonText: '确定',
      cancelButtonText: '取消',
      type: 'warning'
    })
    const res = await invoke<CommandResult<string[]>>('stop_all_apps')
    if (res.code === 0 && res.data) {
      ElMessage.success(`已关闭 ${res.data.length} 个应用`)
      await refreshApps()
    }
  } catch {
    // cancelled
  }
}

async function handleAutoStartApps() {
  try {
    const res = await invoke<CommandResult<string[]>>('start_auto_start_apps')
    if (res.code === 0 && res.data && res.data.length > 0) {
      ElMessage.info(`已自动启动 ${res.data.length} 个应用: ${res.data.join(', ')}`)
      await refreshApps()
    }
  } catch (e) {
    console.error('自动启动失败', e)
  }
}

async function handleExport() {
  try {
    const res = await invoke<CommandResult<string>>('export_config')
    if (res.code === 0 && res.data) {
      const blob = new Blob([res.data], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `app-manager-backup-${new Date().toISOString().slice(0, 10)}.json`
      a.click()
      URL.revokeObjectURL(url)
      ElMessage.success('导出成功')
    }
  } catch (e) {
    ElMessage.error('导出失败')
  }
}

async function handleImport() {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = '.json'
  input.onchange = async (e: Event) => {
    const file = (e.target as HTMLInputElement).files?.[0]
    if (!file) return
    try {
      const text = await file.text()
      const res = await invoke<CommandResult<number>>('import_config', { json: text })
      if (res.code === 0) {
        ElMessage.success(`导入成功，新增 ${res.data} 个应用`)
        await loadFromDisk()
        await fetchApps()
        await fetchGroups()
      } else {
        ElMessage.error(res.msg)
      }
    } catch (e) {
      ElMessage.error('导入失败')
    }
  }
  input.click()
}

function handleEdit(app: AppInstance) {
  editingApp.value = app
  copySource.value = null
  dialogVisible.value = true
}

function handleAdd() {
  editingApp.value = null
  copySource.value = null
  dialogVisible.value = true
}

// 复制为新增：打开添加弹窗并预填被复制应用的配置
function handleCopy(app: AppInstance) {
  editingApp.value = null
  copySource.value = app
  dialogVisible.value = true
}

async function handleDialogSuccess() {
  dialogVisible.value = false
  await fetchApps()
  await fetchGroups()
}

onMounted(async () => {
  initTheme()
  await loadFromDisk()
  await handleAutoStartApps()
  await refreshApps()
  await fetchSystemInfo()
  await fetchGroups()

  refreshTimer = setInterval(() => {
    refreshApps()
    fetchSystemInfo()
  }, 3000)
})

onUnmounted(() => {
  if (refreshTimer) {
    clearInterval(refreshTimer)
    refreshTimer = null
  }
})
</script>

<template>
  <div class="app-manager">
    <header class="page-header">
      <div class="header-brand">
        <div class="brand-icon">
          <el-icon :size="22"><Monitor /></el-icon>
        </div>
        <div class="brand-text">
          <h1>app-manage</h1>
          <span class="brand-sub">管理你的应用与服务</span>
        </div>
      </div>
      <div class="header-actions">
        <div class="action-group">
          <el-tooltip content="切换主题">
            <button class="icon-action" @click="toggleTheme">
              <el-icon :size="18"><Sunny v-if="isDark" /><Moon v-else /></el-icon>
            </button>
          </el-tooltip>
          <el-tooltip content="导出配置">
            <button class="icon-action" @click="handleExport">
              <el-icon :size="18"><Download /></el-icon>
            </button>
          </el-tooltip>
          <el-tooltip content="导入配置">
            <button class="icon-action" @click="handleImport">
              <el-icon :size="18"><Upload /></el-icon>
            </button>
          </el-tooltip>
          <el-tooltip content="刷新">
            <button class="icon-action" :class="{ spinning: loading }" @click="refreshApps">
              <el-icon :size="18"><Refresh /></el-icon>
            </button>
          </el-tooltip>
          <el-tooltip :content="viewMode === 'list' ? '卡片视图' : '列表视图'">
            <button class="icon-action" @click="toggleView">
              <el-icon :size="18"><Grid v-if="viewMode === 'list'" /><List v-else /></el-icon>
            </button>
          </el-tooltip>
          <el-tooltip content="系统工具箱">
            <button class="icon-action" @click="toolKitVisible = true">
              <el-icon :size="18"><SetUp /></el-icon>
            </button>
          </el-tooltip>
        </div>
        <button class="add-btn" @click="handleAdd">
          <el-icon><Plus /></el-icon>
          <span>添加应用</span>
        </button>
      </div>
    </header>

    <section class="system-bar" v-if="systemInfo">
      <div class="sys-card">
        <div class="sys-icon cpu">⚡</div>
        <div class="sys-detail">
          <div class="sys-title">CPU</div>
          <el-progress
            :percentage="Math.min(systemInfo.cpu_usage, 100)"
            :stroke-width="6"
            :show-text="false"
            :color="systemInfo.cpu_usage > 80 ? '#ef4444' : systemInfo.cpu_usage > 50 ? '#f59e0b' : '#22c55e'"
            style="width: 100%"
          />
        </div>
        <div class="sys-value">{{ systemInfo.cpu_usage }}%</div>
      </div>
      <div class="sys-sep" />
      <div class="sys-card">
        <div class="sys-icon mem">💾</div>
        <div class="sys-detail">
          <div class="sys-title">内存</div>
          <el-progress
            :percentage="systemInfo.memory_usage_percent"
            :stroke-width="6"
            :show-text="false"
            :color="systemInfo.memory_usage_percent > 80 ? '#ef4444' : systemInfo.memory_usage_percent > 50 ? '#f59e0b' : '#3b82f6'"
            style="width: 100%"
          />
        </div>
        <div class="sys-value">{{ systemInfo.used_memory_gb }} / {{ systemInfo.total_memory_gb }} GB</div>
      </div>
      <div class="sys-sep" />
      <div class="sys-card compact">
        <div class="app-counts">
          <div class="count-item">
            <span class="count-num">{{ apps.length }}</span>
            <span class="count-label">总计</span>
          </div>
          <div class="count-item active">
            <span class="count-num">{{ runningCount }}</span>
            <span class="count-label">运行</span>
          </div>
          <div class="count-item" v-if="stoppedCount > 0">
            <span class="count-num">{{ stoppedCount }}</span>
            <span class="count-label">停止</span>
          </div>
        </div>
      </div>
    </section>

    <section class="toolbar">
      <div class="toolbar-left">
        <el-input
          v-model="searchText"
          placeholder="搜索应用..."
          :prefix-icon="Search"
          clearable
          class="search-input"
          size="default"
        />
        <div class="filter-group">
          <el-radio-group v-model="statusFilter" size="small">
            <el-radio-button value="all">全部</el-radio-button>
            <el-radio-button value="running">运行中</el-radio-button>
            <el-radio-button value="stopped">已停止</el-radio-button>
          </el-radio-group>
          <el-radio-group v-model="typeFilter" size="small">
            <el-radio-button value="all">所有类型</el-radio-button>
            <el-radio-button value="Command">命令</el-radio-button>
            <el-radio-button value="StaticServer">静态</el-radio-button>
          </el-radio-group>
        </div>
        <el-select
          v-if="groups.length > 0"
          v-model="groupFilter"
          placeholder="分组"
          clearable
          size="small"
          class="group-select"
          @clear="groupFilter = 'all'"
        >
          <el-option label="全部分组" value="all" />
          <el-option v-for="g in groups" :key="g" :label="g" :value="g" />
        </el-select>
      </div>
      <div class="toolbar-right">
        <el-button size="small" @click="handleStartAll" :disabled="stoppedCount === 0">
          <el-icon><VideoPlay /></el-icon> 全部启动
        </el-button>
        <el-button size="small" type="danger" plain @click="handleStopAll" :disabled="runningCount === 0">
          <el-icon><SwitchButton /></el-icon> 全部关闭
        </el-button>
      </div>
    </section>

    <section class="app-grid" v-if="filteredApps.length > 0 && viewMode === 'grid'">
      <AppCard
        v-for="app in filteredApps"
        :key="app.config.id"
        :app="app"
        :dragging="dragId === app.config.id"
        :drop-target="dropId === app.config.id && dragId !== null && dragId !== app.config.id"
        @start="handleStart"
        @stop="handleStop"
        @restart="handleRestart"
        @edit="handleEdit"
        @copy="handleCopy"
        @delete="handleDelete"
        @dragstart="handleDragStart"
        @dragend="handleDragEnd"
        @dragenter="handleDragEnter"
        @drop="handleReorder"
      />
    </section>

    <section class="app-list" v-else-if="filteredApps.length > 0 && viewMode === 'list'">
      <AppListItem
        v-for="app in filteredApps"
        :key="app.config.id"
        :app="app"
        :dragging="dragId === app.config.id"
        :drop-target="dropId === app.config.id && dragId !== null && dragId !== app.config.id"
        @start="handleStart"
        @stop="handleStop"
        @restart="handleRestart"
        @edit="handleEdit"
        @copy="handleCopy"
        @delete="handleDelete"
        @dragstart="handleDragStart"
        @dragend="handleDragEnd"
        @dragenter="handleDragEnter"
        @drop="handleReorder"
      />
    </section>

    <section class="empty-state" v-else-if="apps.length === 0">
      <div class="empty-content">
        <div class="empty-icon">📦</div>
        <h3>还没有添加应用</h3>
        <p>点击下方按钮添加你的第一个应用</p>
        <button class="add-btn large" @click="handleAdd">
          <el-icon><Plus /></el-icon>
          <span>添加应用</span>
        </button>
      </div>
    </section>

    <section class="empty-state" v-else>
      <el-empty description="没有匹配的应用" :image-size="80" />
    </section>

    <footer class="status-bar" v-if="lastRefreshTime">
      <span class="status-text">app-manage v1.0</span>
      <span class="status-dot" />
      <span class="status-text">{{ runningCount }} 运行 · {{ stoppedCount }} 停止</span>
      <span class="status-spacer" />
      <span class="status-text muted">刷新于 {{ lastRefreshTime }}</span>
    </footer>

    <AppFormDialog
      v-model:visible="dialogVisible"
      :editing-app="editingApp"
      :copy-source="copySource"
      :groups="groups"
      @success="handleDialogSuccess"
    />

    <ToolKit v-model:visible="toolKitVisible" />
  </div>
</template>

<style scoped lang="scss">
.app-manager {
  height: 100%;
  padding: 16px 20px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  gap: 10px;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-shrink: 0;
}

.header-brand {
  display: flex;
  align-items: center;
  gap: 10px;

  .brand-icon {
    width: 36px;
    height: 36px;
    background: linear-gradient(135deg, var(--el-color-primary) 0%, #6366f1 100%);
    border-radius: 10px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #fff;
    box-shadow: 0 2px 8px rgba(64, 158, 255, 0.2);
  }

  .brand-text {
    h1 {
      margin: 0;
      font-size: 18px;
      font-weight: 700;
      color: var(--el-text-color-primary);
      line-height: 1.2;
    }

    .brand-sub {
      font-size: 11px;
      color: var(--el-text-color-placeholder);
      font-weight: 400;
    }
  }
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

.action-group {
  display: flex;
  gap: 1px;
  background: var(--el-bg-color);
  border-radius: 8px;
  padding: 2px;
  border: 1px solid var(--el-border-color-extra-light);
}

.icon-action {
  border: none;
  background: transparent;
  cursor: pointer;
  width: 32px;
  height: 32px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-secondary);
  transition: all 0.2s;

  &:hover {
    background: var(--el-fill-color-light);
    color: var(--el-text-color-primary);
  }

  &.spinning .el-icon {
    animation: spin 1s linear infinite;
  }
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.add-btn {
  border: none;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 13px;
  font-weight: 600;
  padding: 6px 14px;
  border-radius: 8px;
  background: linear-gradient(135deg, var(--el-color-primary) 0%, #6366f1 100%);
  color: #fff;
  transition: all 0.25s;
  font-family: inherit;
  box-shadow: 0 2px 6px rgba(64, 158, 255, 0.25);

  &:hover {
    transform: translateY(-1px);
    box-shadow: 0 4px 12px rgba(64, 158, 255, 0.35);
  }

  &.large {
    padding: 10px 24px;
    font-size: 14px;
    border-radius: 10px;
  }
}

.system-bar {
  display: flex;
  align-items: center;
  gap: 0;
  padding: 0;
  background: var(--el-bg-color);
  border-radius: 10px;
  border: 1px solid var(--el-border-color-extra-light);
  flex-shrink: 0;
  overflow: hidden;
}

.sys-card {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 14px;

  &.compact { flex: 0 0 auto; }

  .sys-icon {
    font-size: 18px;
    flex-shrink: 0;
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 8px;

    &.cpu { background: rgba(34, 197, 94, 0.08); }
    &.mem { background: rgba(59, 130, 246, 0.08); }
  }

  .sys-detail {
    flex: 1;
    min-width: 80px;

    .sys-title {
      font-size: 11px;
      color: var(--el-text-color-placeholder);
      font-weight: 500;
      margin-bottom: 3px;
    }
  }

  .sys-value {
    font-size: 12px;
    font-family: 'SF Mono', 'Consolas', monospace;
    font-weight: 600;
    color: var(--el-text-color-regular);
    white-space: nowrap;
    flex-shrink: 0;
  }
}

.app-counts {
  display: flex;
  gap: 12px;

  .count-item {
    text-align: center;

    .count-num {
      display: block;
      font-size: 16px;
      font-weight: 700;
      font-family: 'SF Mono', 'Consolas', monospace;
      color: var(--el-text-color-primary);
      line-height: 1;
    }

    .count-label {
      font-size: 10px;
      color: var(--el-text-color-placeholder);
      margin-top: 2px;
    }

    &.active .count-num { color: #22c55e; }
  }
}

.sys-sep {
  width: 1px;
  align-self: stretch;
  background: var(--el-border-color-extra-light);
  margin: 6px 0;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-shrink: 0;
  flex-wrap: wrap;
  gap: 6px;

  .toolbar-left {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .toolbar-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }
}

.search-input {
  width: 180px;
}

.filter-group {
  display: flex;
  gap: 6px;
}

.group-select {
  width: 120px;
}

.app-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 6px;
  flex: 1;
  overflow-y: auto;
  padding: 2px 0 8px;
  align-content: start;
}

.app-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 2px 0 8px;
}

.empty-state {
  display: flex;
  justify-content: center;
  align-items: center;
  flex: 1;
}

.empty-content {
  text-align: center;
  padding: 30px;

  .empty-icon {
    font-size: 48px;
    margin-bottom: 12px;
    opacity: 0.5;
  }

  h3 {
    font-size: 16px;
    font-weight: 600;
    color: var(--el-text-color-primary);
    margin-bottom: 4px;
  }

  p {
    color: var(--el-text-color-placeholder);
    margin-bottom: 16px;
    font-size: 13px;
  }
}

.status-bar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  background: var(--el-bg-color);
  border-radius: 8px;
  border: 1px solid var(--el-border-color-extra-light);
  flex-shrink: 0;

  .status-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: #22c55e;
  }

  .status-text {
    font-size: 11px;
    color: var(--el-text-color-secondary);
    font-weight: 500;

    &.muted { color: var(--el-text-color-placeholder); }
  }

  .status-spacer { flex: 1; }
}
</style>
