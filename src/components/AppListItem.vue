<script setup lang="ts">
import { computed, ref } from 'vue'
import { MoreFilled, Edit, Delete, VideoPlay, SwitchButton, CloseBold, Document, RefreshRight, Link, TrendCharts, CopyDocument, Loading } from '@element-plus/icons-vue'
import { ElMessage } from 'element-plus'
import { openUrl } from '@tauri-apps/plugin-opener'
import type { AppInstance } from '../App'
import AppLogDialog from './AppLogDialog.vue'
import MonitorChart from './MonitorChart.vue'

const props = defineProps<{
  app: AppInstance
  dragging?: boolean
  dropTarget?: boolean
}>()

const emit = defineEmits<{
  start: [id: string]
  stop: [id: string, force: boolean]
  restart: [id: string]
  edit: [app: AppInstance]
  copy: [app: AppInstance]
  delete: [app: AppInstance]
  dragstart: [id: string]
  dragend: []
  dragenter: [id: string]
  drop: [id: string]
}>()

const logVisible = ref(false)
const monitorVisible = ref(false)

const isStaticServer = computed(() => props.app.config.app_type === 'StaticServer')

const cpuText = computed(() => {
  if (!props.app.process_info) return '-'
  return `${props.app.process_info.cpu_usage}%`
})

const memoryText = computed(() => {
  if (!props.app.process_info) return '-'
  return `${props.app.process_info.memory_mb}MB`
})

const serverUrl = computed(() => {
  if (isStaticServer.value) {
    const port = props.app.server_port || props.app.config.static_server?.port
    return port ? `http://localhost:${port}` : null
  }
  return props.app.config.url || null
})

const runningDuration = computed(() => {
  if (!props.app.started_at) return '-'
  const now = Math.floor(Date.now() / 1000)
  const diff = now - props.app.started_at
  if (diff < 0) return '-'
  const hours = Math.floor(diff / 3600)
  const minutes = Math.floor((diff % 3600) / 60)
  const seconds = diff % 60
  if (hours > 0) return `${hours}h${minutes}m`
  if (minutes > 0) return `${minutes}m${seconds}s`
  return `${seconds}s`
})

const displayCommand = computed(() => {
  if (isStaticServer.value) {
    return `:${props.app.config.static_server?.port || '-'} ${props.app.config.static_server?.root_dir || ''}`
  }
  return props.app.config.command
})

function handleStop() { emit('stop', props.app.config.id, false) }
function handleForceStop() { emit('stop', props.app.config.id, true) }

async function openInBrowser() {
  const url = serverUrl.value
  if (!url) return
  try {
    await openUrl(url)
  } catch {
    ElMessage.error('打开浏览器失败')
  }
}
</script>

<template>
  <div
    class="list-item"
    :class="{ running: app.running, stopped: !app.running, dragging, 'drop-target': dropTarget }"
    draggable="true"
    @dragstart="emit('dragstart', app.config.id)"
    @dragend="emit('dragend')"
    @dragenter.prevent="emit('dragenter', app.config.id)"
    @dragover.prevent
    @drop.prevent.stop="emit('drop', app.config.id)"
  >
    <span class="dot" :class="{ on: app.running, closing: app.stopping }" />

    <span class="color-bar" v-if="app.config.color" :style="{ background: app.config.color }" />

    <div class="col-name" :title="app.config.name">
      {{ app.config.name }}
    </div>

    <div class="col-type">
      <span class="badge" :class="{ static: isStaticServer }">{{ isStaticServer ? '静态' : '命令' }}</span>
    </div>

    <div class="col-cmd" :title="displayCommand">
      <template v-if="isStaticServer">
        :{{ app.config.static_server?.port || '-' }}
      </template>
      <template v-else>
        {{ app.config.command }}
      </template>
    </div>

    <div class="col-group" v-if="app.config.group">
      {{ app.config.group }}
    </div>

    <div class="col-stats" v-if="app.running">
      <span class="tag" v-if="app.pid">PID {{ app.pid }}</span>
      <span class="tag">{{ runningDuration }}</span>
      <span class="tag cpu" v-if="app.process_info">{{ cpuText }}</span>
      <span class="tag mem" v-if="app.process_info">{{ memoryText }}</span>
    </div>

    <div class="col-exit" v-if="!app.running && app.exit_reason" :title="app.exit_reason">
      ⚠ {{ app.exit_reason }}
    </div>

    <div class="col-url" v-if="serverUrl && app.running">
      <a @click.prevent="openInBrowser">{{ serverUrl }}</a>
    </div>

    <div class="col-actions">
      <template v-if="!app.running">
        <button class="btn start" @click="emit('start', app.config.id)"><el-icon><VideoPlay /></el-icon></button>
      </template>
      <template v-else-if="app.stopping">
        <span class="closing-text" title="正在关闭..."><el-icon class="is-loading"><Loading /></el-icon></span>
        <button class="btn kill" v-if="!isStaticServer" @click="handleForceStop" title="强制关闭"><el-icon><CloseBold /></el-icon></button>
      </template>
      <template v-else>
        <button class="btn stop" @click="handleStop" title="关闭"><el-icon><SwitchButton /></el-icon></button>
        <button class="btn kill" v-if="!isStaticServer" @click="handleForceStop" title="强制关闭"><el-icon><CloseBold /></el-icon></button>
        <button class="btn ghost" @click="emit('restart', app.config.id)" title="重启"><el-icon><RefreshRight /></el-icon></button>
        <button class="btn ghost" v-if="serverUrl" @click="openInBrowser" title="打开"><el-icon><Link /></el-icon></button>
      </template>
      <button class="btn ghost" @click="logVisible = true" title="日志"><el-icon><Document /></el-icon></button>
      <button v-if="app.running && app.config.app_type === 'Command'" class="btn ghost" @click="monitorVisible = true" title="监控"><el-icon><TrendCharts /></el-icon></button>
      <el-dropdown trigger="click" size="small">
        <button class="btn ghost"><el-icon><MoreFilled /></el-icon></button>
        <template #dropdown>
          <el-dropdown-menu>
            <el-dropdown-item @click="logVisible = true"><el-icon><Document /></el-icon> 日志</el-dropdown-item>
            <el-dropdown-item v-if="serverUrl && app.running" @click="openInBrowser"><el-icon><Link /></el-icon> 打开</el-dropdown-item>
            <el-dropdown-item @click="emit('copy', app)"><el-icon><CopyDocument /></el-icon> 复制</el-dropdown-item>
            <el-dropdown-item :disabled="app.running" @click="emit('edit', app)"><el-icon><Edit /></el-icon> 编辑</el-dropdown-item>
            <el-dropdown-item :disabled="app.running" divided @click="emit('delete', app)"><el-icon><Delete /></el-icon> 删除</el-dropdown-item>
          </el-dropdown-menu>
        </template>
      </el-dropdown>
    </div>
  </div>

  <AppLogDialog v-model:visible="logVisible" :app="app" />
  <MonitorChart v-model:visible="monitorVisible" :app-id="app.config.id" :app-name="app.config.name" />
</template>

<style scoped lang="scss">
.list-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  background: #fff;
  border: 1px solid rgba(0, 0, 0, 0.04);
  border-radius: 8px;
  transition: all 0.15s;
  min-height: 42px;

  &:hover {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.04);
    background: #fafbfc;
  }

  &.running { border-left: 3px solid #52c41a; }
  &.stopped { opacity: 0.75; }
  &.stopped:hover { opacity: 0.9; }

  &.dragging {
    opacity: 0.5;
  }

  &.drop-target {
    border-color: var(--el-color-primary);
    box-shadow: 0 0 0 2px var(--el-color-primary-light-7);
  }
}

html.dark .list-item {
  background: #1a1a1c;
  border-color: rgba(255, 255, 255, 0.04);
  &:hover { background: #1f1f22; }
  &.running { border-left-color: #52c41a; }
}

.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
  background: #d9d9d9;

  &.on {
    background: #52c41a;
    box-shadow: 0 0 4px rgba(82, 196, 26, 0.5);
    animation: pulse 2s infinite;
  }
  &.closing {
    background: #e6a23c;
    box-shadow: 0 0 4px rgba(230, 162, 60, 0.5);
    animation: pulse 1s infinite;
  }
}

html.dark .dot { background: #4b5563; }

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}

.color-bar {
  width: 3px;
  height: 18px;
  border-radius: 2px;
  flex-shrink: 0;
}

.col-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--el-text-color-primary);
  width: 120px;
  flex-shrink: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col-type {
  flex-shrink: 0;

  .badge {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 6px;
    font-weight: 500;
    line-height: 16px;
    background: rgba(64, 158, 255, 0.07);
    color: #409eff;
    &.static { background: rgba(230, 162, 60, 0.07); color: #e6a23c; }
  }
}

html.dark .col-type .badge { background: rgba(64, 158, 255, 0.1); &.static { background: rgba(230, 162, 60, 0.1); } }

.col-cmd {
  font-size: 11px;
  font-family: 'SF Mono', 'Consolas', monospace;
  color: var(--el-text-color-placeholder);
  flex: 1;
  min-width: 100px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col-group {
  font-size: 11px;
  color: var(--el-text-color-placeholder);
  flex-shrink: 0;
  max-width: 80px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col-stats {
  display: flex;
  gap: 6px;
  flex-shrink: 0;

  .tag {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--el-fill-color-extra-light);
    color: var(--el-text-color-secondary);
    font-family: 'SF Mono', 'Consolas', monospace;
    white-space: nowrap;

    &.cpu { color: var(--el-color-primary); }
    &.mem { color: var(--el-color-warning); }
  }
}

.col-exit {
  font-size: 11px;
  color: var(--el-color-warning-dark-2);
  flex: 1;
  min-width: 60px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col-url {
  flex-shrink: 0;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;

  a {
    font-size: 11px;
    color: var(--el-color-primary);
    text-decoration: none;
    cursor: pointer;
    &:hover { text-decoration: underline; }
  }
}

.col-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
  margin-left: auto;
}

.closing-text {
  width: 26px;
  height: 26px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-color-warning);
}

.btn {
  border: none;
  background: transparent;
  cursor: pointer;
  width: 26px;
  height: 26px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-placeholder);
  transition: all 0.15s;
  font-size: 13px;
  padding: 0;

  &:hover { background: var(--el-fill-color-light); color: var(--el-text-color-regular); }

  &.start { color: var(--el-color-success); &:hover { background: rgba(82, 196, 26, 0.08); } }
  &.stop { color: var(--el-color-warning); &:hover { background: rgba(230, 162, 60, 0.08); } }
  &.kill { color: var(--el-color-danger); &:hover { background: rgba(245, 108, 108, 0.08); } }
}
</style>
