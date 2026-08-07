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
const appTypeText = computed(() => isStaticServer.value ? '静态' : '命令')

const statusDot = computed(() => props.app.stopping ? 'closing' : (props.app.running ? 'running' : 'stopped'))

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

const colorVar = computed(() => ({
  '--accent': props.app.config.color || 'var(--el-color-primary)'
}))

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
    class="app-card"
    :class="{ running: app.running, stopped: !app.running, dragging, 'drop-target': dropTarget }"
    :style="colorVar"
    draggable="true"
    @dragstart="emit('dragstart', app.config.id)"
    @dragend="emit('dragend')"
    @dragenter.prevent="emit('dragenter', app.config.id)"
    @dragover.prevent
    @drop.prevent.stop="emit('drop', app.config.id)"
  >
    <div class="card-accent" />

    <div class="card-row">
      <div class="card-left">
        <div class="name-row">
          <span class="dot" :class="statusDot" />
          <span class="app-name">{{ app.config.name }}</span>
          <span class="badge type" :class="{ static: isStaticServer }">{{ appTypeText }}</span>
          <span class="badge group" v-if="app.config.group">{{ app.config.group }}</span>
        </div>
        <div class="sub-row">
          <template v-if="isStaticServer">
            <span class="sub-item">:{{ app.config.static_server?.port || '-' }}</span>
          </template>
          <template v-else>
            <el-tooltip :content="app.config.command" placement="top">
              <span class="sub-item cmd">{{ app.config.command }}</span>
            </el-tooltip>
          </template>
          <span class="sub-item" v-if="app.config.description">{{ app.config.description }}</span>
        </div>
      </div>
      <el-dropdown trigger="click" class="card-more" size="small">
        <button class="more-btn"><el-icon :size="14"><MoreFilled /></el-icon></button>
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

    <div class="exit-banner" v-if="!app.running && app.exit_reason">
      ⚠ {{ app.exit_reason }}
    </div>

    <div class="card-stats" v-if="app.running">
      <div class="stats-row">
        <span class="stat" v-if="app.pid">PID <b>{{ app.pid }}</b></span>
        <span class="stat" v-if="app.started_at">{{ runningDuration }}</span>
        <span class="stat" v-if="app.process_info">CPU <b class="cpu">{{ cpuText }}</b></span>
        <span class="stat" v-if="app.process_info">MEM <b class="mem">{{ memoryText }}</b></span>
        <a v-if="serverUrl" :href="serverUrl" target="_blank" class="stat link" @click.stop>{{ serverUrl }}</a>
      </div>
    </div>

    <div class="card-actions">
      <template v-if="!app.running">
        <button class="btn start" @click="emit('start', app.config.id)"><el-icon><VideoPlay /></el-icon> 启动</button>
      </template>
      <template v-else-if="app.stopping">
        <span class="closing-text"><el-icon class="is-loading"><Loading /></el-icon> 正在关闭...</span>
        <button class="btn kill" v-if="!isStaticServer" @click="handleForceStop"><el-icon><CloseBold /></el-icon></button>
      </template>
      <template v-else>
        <button class="btn stop" @click="handleStop"><el-icon><SwitchButton /></el-icon> 关闭</button>
        <button class="btn kill" v-if="!isStaticServer" @click="handleForceStop"><el-icon><CloseBold /></el-icon></button>
        <button class="btn icon" @click="emit('restart', app.config.id)" title="重启"><el-icon><RefreshRight /></el-icon></button>
        <button class="btn icon" v-if="serverUrl" @click="openInBrowser" title="打开"><el-icon><Link /></el-icon></button>
      </template>
      <span class="grow" />
      <button v-if="app.running && app.config.app_type === 'Command'" class="btn icon" @click="monitorVisible = true" title="监控"><el-icon><TrendCharts /></el-icon></button>
      <button class="btn icon" @click="logVisible = true" title="日志"><el-icon><Document /></el-icon></button>
    </div>
  </div>

  <AppLogDialog v-model:visible="logVisible" :app="app" />
  <MonitorChart v-model:visible="monitorVisible" :app-id="app.config.id" :app-name="app.config.name" />
</template>

<style scoped lang="scss">
.app-card {
  position: relative;
  background: #fff;
  border-radius: 8px;
  border: 1px solid rgba(0, 0, 0, 0.06);
  padding: 8px 10px 6px;
  display: flex;
  flex-direction: column;
  gap: 5px;
  transition: all 0.2s ease;
  overflow: hidden;

  &:hover {
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
    border-color: rgba(0, 0, 0, 0.1);
  }

  &.running {
    border-color: rgba(82, 196, 26, 0.2);
    background: linear-gradient(180deg, rgba(82, 196, 26, 0.02) 0%, #fff 100%);
  }
  &.running:hover {
    box-shadow: 0 2px 12px rgba(82, 196, 26, 0.08);
    border-color: rgba(82, 196, 26, 0.3);
  }
  &.stopped { opacity: 0.85; }
  &.stopped:hover { opacity: 1; }

  &.dragging {
    opacity: 0.5;
    transform: scale(0.98);
  }

  &.drop-target {
    border-color: var(--el-color-primary);
    box-shadow: 0 0 0 2px var(--el-color-primary-light-7);
  }
}

html.dark .app-card {
  background: #1a1a1c;
  border-color: rgba(255, 255, 255, 0.06);
  &:hover { border-color: rgba(255, 255, 255, 0.1); }
  &.running {
    background: linear-gradient(180deg, rgba(82, 196, 26, 0.03) 0%, #1a1a1c 100%);
    border-color: rgba(82, 196, 26, 0.15);
  }
  &.running:hover { border-color: rgba(82, 196, 26, 0.25); }
}

.card-accent {
  position: absolute;
  top: 0;
  left: 0;
  width: 3px;
  height: 100%;
  background: var(--accent);
  opacity: 0.7;
  border-radius: 0 2px 2px 0;
}

.card-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 6px;
}

.card-left {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.name-row {
  display: flex;
  align-items: center;
  gap: 5px;

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;

    &.running {
      background: #52c41a;
      box-shadow: 0 0 5px rgba(82, 196, 26, 0.5);
      animation: pulse 2s infinite;
    }
    &.stopped { background: #d4d4d4; }
    &.closing {
      background: #e6a23c;
      box-shadow: 0 0 5px rgba(230, 162, 60, 0.5);
      animation: pulse 1s infinite;
    }
  }

  .app-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--el-text-color-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1.3;
  }
}

html.dark .name-row .dot.stopped { background: #4b5563; }

.badge {
  font-size: 10px;
  padding: 0 5px;
  border-radius: 4px;
  font-weight: 500;
  line-height: 16px;
  white-space: nowrap;
  flex-shrink: 0;

  &.type {
    background: rgba(64, 158, 255, 0.08);
    color: #409eff;
    &.static { background: rgba(230, 162, 60, 0.08); color: #e6a23c; }
  }
  &.group {
    background: rgba(144, 147, 153, 0.08);
    color: var(--el-text-color-secondary);
  }
}

html.dark .badge.type { background: rgba(64, 158, 255, 0.12); &.static { background: rgba(230, 162, 60, 0.12); } }
html.dark .badge.group { background: rgba(144, 147, 153, 0.1); }

.sub-row {
  display: flex;
  gap: 6px;
  align-items: center;
  min-width: 0;
  overflow: hidden;

  .sub-item {
    font-size: 11px;
    color: var(--el-text-color-placeholder);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex-shrink: 1;

    &.cmd {
      font-family: 'SF Mono', 'Consolas', monospace;
      background: var(--el-fill-color-extra-light);
      padding: 1px 5px;
      border-radius: 3px;
      font-size: 10px;
      max-width: 180px;
    }
  }
}

.more-btn {
  border: none;
  background: transparent;
  cursor: pointer;
  width: 24px;
  height: 24px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--el-text-color-placeholder);
  transition: all 0.15s;
  flex-shrink: 0;

  &:hover { background: var(--el-fill-color-light); color: var(--el-text-color-regular); }
}

.exit-banner {
  font-size: 10px;
  color: var(--el-color-warning-dark-2);
  padding: 4px 8px;
  background: rgba(250, 173, 20, 0.06);
  border: 1px solid rgba(250, 173, 20, 0.12);
  border-radius: 5px;
  line-height: 1.3;
}

html.dark .exit-banner { background: rgba(250, 173, 20, 0.08); }

.card-stats {
  border-radius: 5px;
  padding: 5px 8px;
  background: var(--el-fill-color-extra-light);
}

.stats-row {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  align-items: center;
  font-size: 11px;

  .stat {
    color: var(--el-text-color-placeholder);
    white-space: nowrap;

    b {
      color: var(--el-text-color-primary);
      font-family: 'SF Mono', 'Consolas', monospace;
      font-weight: 600;

      &.cpu { color: var(--el-color-primary); }
      &.mem { color: var(--el-color-warning); }
    }

    &.link {
      color: var(--el-color-primary);
      text-decoration: none;
      cursor: pointer;
      &:hover { text-decoration: underline; }
    }
  }
}

.card-actions {
  display: flex;
  gap: 4px;
  align-items: center;
  padding-top: 5px;
  border-top: 1px solid var(--el-border-color-extra-light);
}

.grow { flex: 1; }

.closing-text {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 11px;
  color: var(--el-color-warning);
}

.btn {
  border: none;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 11px;
  font-weight: 500;
  padding: 3px 8px;
  border-radius: 5px;
  transition: all 0.15s;
  font-family: inherit;
  line-height: 1.3;

  &.start {
    background: var(--el-color-success);
    color: #fff;
    &:hover { background: var(--el-color-success-dark-2); }
  }
  &.stop {
    background: rgba(230, 162, 60, 0.08);
    color: var(--el-color-warning-dark-2);
    &:hover { background: rgba(230, 162, 60, 0.15); }
  }
  &.kill {
    background: rgba(245, 108, 108, 0.06);
    color: var(--el-color-danger);
    &:hover { background: rgba(245, 108, 108, 0.12); }
  }
  &.icon {
    background: transparent;
    color: var(--el-text-color-placeholder);
    padding: 3px 5px;
    &:hover { background: var(--el-fill-color-light); color: var(--el-text-color-regular); }
  }
}
</style>
