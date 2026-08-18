<script setup lang="ts">
import { ref, watch, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

defineOptions({ name: 'MonitorChart' })
import { ElSwitch, ElMessage } from 'element-plus'
import type { CommandResult, MetricPoint } from '../App'

type TabKey = 'cpu' | 'mem' | 'net'

const props = defineProps<{
  visible: boolean
  appId: string
  appName: string
}>()

const emit = defineEmits<{
  'update:visible': [val: boolean]
}>()

const metrics = ref<MetricPoint[]>([])
const canvasRef = ref<HTMLCanvasElement>()
const monitoringOn = ref(true)
const timeRange = ref<3600 | 1800 | 600>(3600)
const activeTab = ref<TabKey>('cpu')
let fetchTimer: ReturnType<typeof setInterval> | null = null

async function fetchMonitoringState() {
  try {
    const res = await invoke<CommandResult<boolean>>('get_monitoring')
    if (res.code === 0 && res.data !== null) {
      monitoringOn.value = res.data
    }
  } catch (e) {
    console.error('获取监控状态失败', e)
  }
}

async function toggleMonitoring(val: string | number | boolean) {
  const enabled = !!val
  try {
    await invoke<CommandResult<boolean>>('toggle_monitoring', { enabled })
    monitoringOn.value = enabled
    if (!enabled) {
      metrics.value = []
      drawChart()
    }
  } catch {
    monitoringOn.value = !enabled
    ElMessage.error('切换监控失败')
  }
}

async function fetchMetrics() {
  if (!props.appId || !props.visible) return
  try {
    const res = await invoke<CommandResult<MetricPoint[]>>('get_metrics', { id: props.appId })
    if (res.code === 0 && res.data) {
      const now = Math.floor(Date.now() / 1000)
      const cutoff = now - timeRange.value
      metrics.value = res.data.filter(p => p.ts >= cutoff)
      drawChart()
    }
  } catch (e) {
    console.error('获取监控数据失败', e)
  }
}

function formatBytes(v: number): string {
  if (v >= 1024 * 1024 * 1024) return `${(v / 1024 / 1024 / 1024).toFixed(1)}G`
  if (v >= 1024 * 1024) return `${(v / 1024 / 1024).toFixed(1)}M`
  return `${(v / 1024).toFixed(0)}K`
}

function formatRate(v: number): string {
  if (v >= 1024 * 1024) return `${(v / 1024 / 1024).toFixed(1)}MB/s`
  if (v >= 1024) return `${(v / 1024).toFixed(1)}KB/s`
  return `${v.toFixed(0)}B/s`
}

function drawChart() {
  const canvas = canvasRef.value
  if (!canvas) return
  const ctx = canvas.getContext('2d')
  if (!ctx) return

  const dpr = window.devicePixelRatio || 1
  const rect = canvas.getBoundingClientRect()
  canvas.width = rect.width * dpr
  canvas.height = rect.height * dpr
  ctx.scale(dpr, dpr)

  const W = rect.width
  const H = rect.height
  const padL = 56
  const padR = 16
  const padT = 16
  const padB = 30
  const chartW = W - padL - padR
  const chartH = H - padT - padB

  ctx.clearRect(0, 0, W, H)
  ctx.fillStyle = '#0d1117'
  ctx.fillRect(0, 0, W, H)

  ctx.strokeStyle = 'rgba(255,255,255,0.06)'
  ctx.lineWidth = 1
  for (let i = 0; i <= 4; i++) {
    const y = padT + (chartH / 4) * i
    ctx.beginPath()
    ctx.moveTo(padL, y)
    ctx.lineTo(padL + chartW, y)
    ctx.stroke()
  }

  if (metrics.value.length < 2) {
    ctx.fillStyle = '#484f58'
    ctx.font = '13px sans-serif'
    ctx.textAlign = 'center'
    ctx.fillText('暂无监控数据', W / 2, H / 2)
    return
  }

  const data = metrics.value
  const now = Math.floor(Date.now() / 1000)
  const rangeStart = now - timeRange.value

  const scaleX = (ts: number) => padL + ((ts - rangeStart) / timeRange.value) * chartW
  const bottomY = padT + chartH

  if (activeTab.value === 'cpu') {
    const maxVal = Math.max(10, ...data.map(p => p.cpu))
    const scaleY = (v: number) => padT + chartH - (v / maxVal) * chartH
    drawLine(ctx, data, scaleX, scaleY, '#58a6ff', 'rgba(88,166,255,0.08)', (p) => p.cpu, bottomY)
    drawAxesLabels(ctx, padL, padT, chartH, maxVal, (v) => `${v.toFixed(0)}%`, '#58a6ff')
  } else if (activeTab.value === 'mem') {
    const maxVal = Math.max(10 * 1024 * 1024, ...data.map(p => p.mem))
    const scaleY = (v: number) => padT + chartH - (v / maxVal) * chartH
    drawLine(ctx, data, scaleX, scaleY, '#f0883e', 'rgba(240,136,62,0.08)', (p) => p.mem, bottomY)
    drawAxesLabels(ctx, padL, padT, chartH, maxVal, formatBytes, '#f0883e')
  } else {
    const maxVal = Math.max(1024, data.reduce((acc, p) => Math.max(acc, p.net_in, p.net_out), 0))
    const scaleY = (v: number) => padT + chartH - (v / maxVal) * chartH
    drawLine(ctx, data, scaleX, scaleY, '#3fb950', 'rgba(63,185,80,0.08)', (p) => p.net_in, bottomY)
    drawLine(ctx, data, scaleX, scaleY, '#d29922', 'rgba(210,153,34,0.08)', (p) => p.net_out, bottomY)
    drawAxesLabels(ctx, padL, padT, chartH, maxVal, formatRate, '#3fb950')

    ctx.font = '11px sans-serif'
    ctx.textAlign = 'right'
    ctx.fillStyle = '#3fb950'
    ctx.fillText('— 下行', W - padR, padT + 12)
    ctx.fillStyle = '#d29922'
    ctx.fillText('— 上行', W - padR, padT + 26)
  }

  const timeLabels = timeRange.value === 3600 ? 6 : timeRange.value === 1800 ? 6 : 5
  const step = timeRange.value / timeLabels
  ctx.fillStyle = '#484f58'
  ctx.font = '10px sans-serif'
  ctx.textAlign = 'center'
  for (let i = 0; i <= timeLabels; i++) {
    const ts = rangeStart + step * i
    const x = scaleX(ts)
    const mins = Math.round((now - ts) / 60)
    const label = mins === 0 ? '现在' : `-${mins}m`
    ctx.fillText(label, x, padT + chartH + 18)
  }
}

function drawLine(
  ctx: CanvasRenderingContext2D,
  data: MetricPoint[],
  scaleX: (ts: number) => number,
  scaleY: (v: number) => number,
  color: string,
  fillColor: string,
  getValue: (p: MetricPoint) => number,
  bottomY: number
) {
  if (data.length < 2) return
  ctx.beginPath()
  ctx.moveTo(scaleX(data[0].ts), scaleY(getValue(data[0])))
  for (let i = 1; i < data.length; i++) {
    ctx.lineTo(scaleX(data[i].ts), scaleY(getValue(data[i])))
  }
  ctx.strokeStyle = color
  ctx.lineWidth = 1.5
  ctx.stroke()

  ctx.lineTo(scaleX(data[data.length - 1].ts), bottomY)
  ctx.lineTo(scaleX(data[0].ts), bottomY)
  ctx.closePath()
  ctx.fillStyle = fillColor
  ctx.fill()
}

function drawAxesLabels(
  ctx: CanvasRenderingContext2D,
  padL: number,
  padT: number,
  chartH: number,
  maxVal: number,
  fmt: (v: number) => string,
  color: string
) {
  ctx.font = '10px sans-serif'
  ctx.textAlign = 'right'
  for (let i = 0; i <= 4; i++) {
    const y = padT + (chartH / 4) * i
    const val = maxVal - (maxVal / 4) * i
    ctx.fillStyle = color
    ctx.fillText(fmt(val), padL - 6, y + 3)
  }
}

watch(() => props.visible, async (val) => {
  if (val) {
    await fetchMonitoringState()
    await fetchMetrics()
    fetchTimer = setInterval(fetchMetrics, 3000)
  } else {
    if (fetchTimer) {
      clearInterval(fetchTimer)
      fetchTimer = null
    }
  }
})

watch(timeRange, () => {
  fetchMetrics()
})

watch(activeTab, () => {
  drawChart()
})

onUnmounted(() => {
  if (fetchTimer) {
    clearInterval(fetchTimer)
    fetchTimer = null
  }
})

function handleClose() {
  emit('update:visible', false)
}
</script>

<template>
  <el-dialog
    :model-value="visible"
    :title="`监控 - ${appName}`"
    width="720px"
    @close="handleClose"
    destroy-on-close
    top="8vh"
  >
    <div class="monitor-toolbar">
      <div class="toolbar-left">
        <span class="toolbar-label">监控采集</span>
        <el-switch v-model="monitoringOn" @change="toggleMonitoring" size="small" />
      </div>
      <div class="toolbar-right">
        <button
          v-for="r in ([3600, 1800, 600] as const)"
          :key="r"
          class="range-btn"
          :class="{ active: timeRange === r }"
          @click="timeRange = r"
        >
          {{ r === 3600 ? '1小时' : r === 1800 ? '30分钟' : '10分钟' }}
        </button>
      </div>
    </div>

    <el-tabs v-model="activeTab" class="monitor-tabs">
      <el-tab-pane label="CPU" name="cpu" />
      <el-tab-pane label="内存" name="mem" />
      <el-tab-pane label="网络" name="net" />
    </el-tabs>

    <div class="chart-wrapper">
      <canvas ref="canvasRef" class="chart-canvas" />
    </div>

    <div class="chart-summary">
      <template v-if="metrics.length > 0">
        <span class="summary-item">数据点: {{ metrics.length }}</span>
        <template v-if="activeTab === 'cpu'">
          <span class="summary-item">最新 CPU: {{ metrics[metrics.length - 1].cpu.toFixed(2) }}%</span>
        </template>
        <template v-else-if="activeTab === 'mem'">
          <span class="summary-item">最新内存: {{ formatBytes(metrics[metrics.length - 1].mem) }}</span>
        </template>
        <template v-else>
          <span class="summary-item">下行: {{ formatRate(metrics[metrics.length - 1].net_in) }}</span>
          <span class="summary-item">上行: {{ formatRate(metrics[metrics.length - 1].net_out) }}</span>
        </template>
      </template>
    </div>

    <template #footer>
      <el-button @click="handleClose">关闭</el-button>
    </template>
  </el-dialog>
</template>

<style scoped lang="scss">
:deep(.el-dialog__header) {
  padding: 16px 24px 12px;
  border-bottom: 1px solid var(--el-border-color-extra-light);
  margin-right: 0;
}

:deep(.el-dialog__body) {
  padding: 16px 24px;
}

:deep(.el-dialog__footer) {
  padding: 12px 24px;
  border-top: 1px solid var(--el-border-color-extra-light);
}

.monitor-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 14px;
  padding: 8px 14px;
  background: var(--el-fill-color-extra-light);
  border-radius: 10px;

  .toolbar-left {
    display: flex;
    align-items: center;
    gap: 8px;

    .toolbar-label {
      font-size: 13px;
      color: var(--el-text-color-regular);
    }
  }

  .toolbar-right {
    display: flex;
    gap: 4px;

    .range-btn {
      padding: 3px 12px;
      font-size: 12px;
      border: 1px solid var(--el-border-color);
      border-radius: 6px;
      background: transparent;
      color: var(--el-text-color-regular);
      cursor: pointer;
      transition: all 0.2s;

      &:hover {
        border-color: var(--el-color-primary);
        color: var(--el-color-primary);
      }

      &.active {
        background: var(--el-color-primary);
        border-color: var(--el-color-primary);
        color: #fff;
      }
    }
  }
}

.monitor-tabs {
  margin-bottom: 8px;

  :deep(.el-tabs__header) {
    margin-bottom: 10px;
  }

  :deep(.el-tabs__nav-wrap)::after {
    height: 1px;
  }
}

.chart-wrapper {
  border-radius: 12px;
  overflow: hidden;
  border: 1px solid rgba(255, 255, 255, 0.06);
}

.chart-canvas {
  display: block;
  width: 100%;
  height: 320px;
  background: #0d1117;
}

.chart-summary {
  display: flex;
  gap: 20px;
  margin-top: 10px;
  padding: 0 4px;

  .summary-item {
    font-size: 12px;
    color: var(--el-text-color-secondary);
  }
}
</style>
