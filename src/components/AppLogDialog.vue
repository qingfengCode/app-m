<script setup lang="ts">
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { ElMessage } from 'element-plus'
import { Delete } from '@element-plus/icons-vue'
import type { AppInstance, LogEntry, CommandResult } from '../App'

const props = defineProps<{
  visible: boolean
  app: AppInstance | null
}>()

const emit = defineEmits<{
  'update:visible': [val: boolean]
}>()

const logs = ref<LogEntry[]>([])
const loading = ref(false)
const autoScroll = ref(true)
const logContainer = ref<HTMLElement>()

async function fetchLogs(showLoading = false) {
  if (!props.app) return
  if (showLoading) loading.value = true
  try {
    const res = await invoke<CommandResult<LogEntry[]>>('get_app_logs', { id: props.app.config.id })
    if (res.code === 0 && res.data) {
      logs.value = res.data
      if (autoScroll.value) {
        setTimeout(() => {
          if (logContainer.value) {
            logContainer.value.scrollTop = logContainer.value.scrollHeight
          }
        }, 50)
      }
    }
  } catch (e) {
    console.error('获取日志失败', e)
  } finally {
    if (showLoading) loading.value = false
  }
}

async function handleClear() {
  if (!props.app) return
  try {
    const res = await invoke<CommandResult<null>>('clear_app_logs', { id: props.app.config.id })
    if (res.code === 0) {
      logs.value = []
      ElMessage.success('日志已清空')
    }
  } catch (e) {
    ElMessage.error('清空日志失败')
  }
}

function formatTime(ts: number) {
  const d = new Date(ts * 1000)
  return d.toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit'
  })
}

function levelClass(level: string) {
  switch (level) {
    case 'error': return 'log-error'
    case 'warn': return 'log-warn'
    case 'info': return 'log-info'
    default: return 'log-default'
  }
}

// 16 色基础色板（映射 ANSI 30-37 / 90-97 / 40-47 / 100-107）
const ANSI_PALETTE = [
  '#000000', '#800000', '#008000', '#808000',
  '#000080', '#800080', '#008080', '#c0c0c0',
  '#808080', '#ff0000', '#00ff00', '#ffff00',
  '#0000ff', '#ff00ff', '#00ffff', '#ffffff'
]

function ansi256Color(n: number): string {
  if (n < 16) return ANSI_PALETTE[n]
  if (n < 232) {
    // 6x6x6 彩色立方体
    const idx = n - 16
    const r = Math.floor(idx / 36)
    const g = Math.floor((idx % 36) / 6)
    const b = idx % 6
    const conv = (v: number) => (v === 0 ? 0 : 55 + v * 40)
    return `rgb(${conv(r)},${conv(g)},${conv(b)})`
  }
  // 灰度
  const g = 8 + (n - 232) * 10
  return `rgb(${g},${g},${g})`
}

const ANSI_SGR = /\x1b\[([0-9;]*)m/g

/**
 * 将 ANSI 转义序列转换为彩色 HTML。
 * 先转义 HTML 实体再注入受控的 <span>，避免子进程输出被当作 HTML 注入。
 */
function ansiToHtml(text: string): string {
  let html = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
  let opened = 0
  html = html.replace(ANSI_SGR, (_m, codes: string) => {
    const nums = codes ? codes.split(';').map(Number) : [0]
    if (nums.includes(0)) {
      // 重置：关闭所有已打开的 span
      const close = '</span>'.repeat(opened)
      opened = 0
      return close
    }
    let style = ''
    let i = 0
    while (i < nums.length) {
      const n = nums[i]
      if (n === 1) style += 'font-weight:bold;'
      else if (n === 2) style += 'opacity:.65;'
      else if (n === 3) style += 'font-style:italic;'
      else if (n === 4) style += 'text-decoration:underline;'
      else if (n === 9) style += 'text-decoration:line-through;'
      else if (n === 38 && nums[i + 1] === 5) { style += `color:${ansi256Color(nums[i + 2])};`; i += 2 }
      else if (n === 38 && nums[i + 1] === 2) { style += `color:rgb(${nums[i + 2]},${nums[i + 3]},${nums[i + 4]});`; i += 4 }
      else if (n === 48 && nums[i + 1] === 5) { style += `background-color:${ansi256Color(nums[i + 2])};`; i += 2 }
      else if (n === 48 && nums[i + 1] === 2) { style += `background-color:rgb(${nums[i + 2]},${nums[i + 3]},${nums[i + 4]});`; i += 4 }
      else if (n >= 30 && n <= 37) style += `color:${ANSI_PALETTE[n - 30]};`
      else if (n >= 90 && n <= 97) style += `color:${ANSI_PALETTE[n - 90 + 8]};`
      else if (n >= 40 && n <= 47) style += `background-color:${ANSI_PALETTE[n - 40]};`
      else if (n >= 100 && n <= 107) style += `background-color:${ANSI_PALETTE[n - 100 + 8]};`
      i++
    }
    if (!style) return ''
    opened++
    return `<span style="${style}">`
  })
  return html + '</span>'.repeat(opened)
}

let logTimer: ReturnType<typeof setInterval> | null = null
watch(() => props.visible, (val) => {
  if (val) {
    if (props.app) fetchLogs(true)
    logTimer = setInterval(fetchLogs, 2000)
  } else {
    if (logTimer) {
      clearInterval(logTimer)
      logTimer = null
    }
  }
})

function handleClose() {
  emit('update:visible', false)
}
</script>

<template>
  <el-dialog
    :model-value="visible"
    :title="`日志 - ${app?.config.name || ''}`"
    width="700px"
    @close="handleClose"
    destroy-on-close
    top="8vh"
  >
    <div class="log-toolbar">
      <el-checkbox v-model="autoScroll" size="small">自动滚动</el-checkbox>
      <el-button size="small" :icon="Delete" @click="handleClear" type="danger" plain>
        清空日志
      </el-button>
    </div>

    <div class="log-container" ref="logContainer" v-loading="loading">
      <div v-if="logs.length === 0" class="log-empty">暂无日志</div>
      <div
        v-for="(log, idx) in logs"
        :key="idx"
        class="log-line"
        :class="levelClass(log.level)"
      >
        <span class="log-time">{{ formatTime(log.timestamp) }}</span>
        <span class="log-level">[{{ log.level.toUpperCase() }}]</span>
        <span class="log-content" v-html="ansiToHtml(log.content)"></span>
      </div>
    </div>

    <template #footer>
      <el-button @click="handleClose">关闭</el-button>
    </template>
  </el-dialog>
</template>

<style scoped lang="scss">
:deep(.el-dialog__header) {
  padding: 18px 24px 14px;
  border-bottom: 1px solid var(--el-border-color-extra-light);
  margin-right: 0;
}

:deep(.el-dialog__body) {
  padding: 16px 24px;
}

:deep(.el-dialog__footer) {
  padding: 14px 24px;
  border-top: 1px solid var(--el-border-color-extra-light);
}

.log-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
  padding: 8px 12px;
  background: var(--el-fill-color-extra-light);
  border-radius: 10px;
}

.log-container {
  background: #0d1117;
  border-radius: 12px;
  padding: 14px;
  height: 460px;
  overflow-y: auto;
  font-family: 'SF Mono', 'Consolas', 'Monaco', 'Courier New', monospace;
  font-size: 10px;
  line-height: 1.5;
  border: 1px solid rgba(255, 255, 255, 0.06);

  .log-empty {
    color: #484f58;
    text-align: center;
    padding-top: 180px;
    font-size: 13px;
  }

  .log-line {
    display: flex;
    gap: 10px;
    padding: 2px 6px;
    border-radius: 4px;
    transition: background 0.15s;

    &:hover { background: rgba(255, 255, 255, 0.03); }

    .log-time {
      color: #484f58;
      flex-shrink: 0;
      font-size: 10px;
    }

    .log-level {
      flex-shrink: 0;
      font-weight: 700;
      min-width: 52px;
      font-size: 10px;
    }

    .log-content {
      color: #c9d1d9;
      word-break: break-all;
    }
  }

  .log-info {
    .log-level { color: #58a6ff; }
  }

  .log-warn {
    background: rgba(210, 153, 34, 0.05);
    .log-level { color: #d29922; }
    .log-content { color: #e3b341; }
  }

  .log-error {
    background: rgba(248, 81, 73, 0.06);
    .log-level { color: #f85149; }
    .log-content { color: #ffa198; }
  }

  .log-default {
    .log-level { color: #8b949e; }
  }
}
</style>
