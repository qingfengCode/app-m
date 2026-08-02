<script setup lang="ts">
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { ElMessage, ElMessageBox } from 'element-plus'
import { Search, Delete, Position, RefreshRight, Plus } from '@element-plus/icons-vue'
import type { CommandResult, ProcessItem, PortMapping, HostsEntry } from '../App'

defineProps<{
  visible: boolean
}>()

const emit = defineEmits<{
  'update:visible': [val: boolean]
}>()

const activeTab = ref('process')

const processKeyword = ref('')
const processResults = ref<ProcessItem[]>([])
const processLoading = ref(false)

const portNumber = ref<number | undefined>(undefined)
const portResults = ref<PortMapping[]>([])
const portLoading = ref(false)

const killPid = ref<number | undefined>(undefined)
const killLoading = ref(false)

const hostsEntries = ref<HostsEntry[]>([])
const hostsLoading = ref(false)
const hostsSaving = ref(false)
const hostsEditing = ref(false)
const hostsAddForm = ref({ ip: '', host: '' })

async function findProcess() {
  if (!processKeyword.value.trim()) {
    ElMessage.warning('请输入进程名关键词')
    return
  }
  processLoading.value = true
  try {
    const res = await invoke<CommandResult<ProcessItem[]>>('tool_find_process', { keyword: processKeyword.value })
    if (res.code === 0 && res.data) {
      processResults.value = res.data
      if (res.data.length === 0) {
        ElMessage.info('未找到匹配的进程')
      }
    }
  } catch (e) {
    ElMessage.error('查找失败')
  } finally {
    processLoading.value = false
  }
}

async function findPort() {
  if (!portNumber.value) {
    ElMessage.warning('请输入端口号')
    return
  }
  portLoading.value = true
  try {
    const res = await invoke<CommandResult<PortMapping[]>>('tool_find_port', { port: portNumber.value })
    if (res.code === 0 && res.data) {
      portResults.value = res.data
      if (res.data.length === 0) {
        ElMessage.info(`端口 ${portNumber.value} 未被占用`)
      }
    }
  } catch (e) {
    ElMessage.error('查找失败')
  } finally {
    portLoading.value = false
  }
}

async function handleKill(pid?: number) {
  const targetPid = pid ?? killPid.value
  if (!targetPid) {
    ElMessage.warning('请输入要终止的 PID')
    return
  }
  try {
    await ElMessageBox.confirm(
      `确定要终止进程 PID: ${targetPid} 吗？此操作不可恢复。`,
      '确认终止进程',
      { confirmButtonText: '终止', cancelButtonText: '取消', type: 'warning' }
    )
  } catch {
    return
  }

  killLoading.value = true
  try {
    const res = await invoke<CommandResult<string>>('tool_kill_pid', { pid: targetPid })
    if (res.code === 0) {
      ElMessage.success(res.data || '进程已终止')
      killPid.value = undefined
      processResults.value = processResults.value.filter(p => p.pid !== targetPid)
      portResults.value = portResults.value.filter(p => p.pid !== targetPid)
    } else {
      ElMessage.error(res.msg)
    }
  } catch (e) {
    ElMessage.error('终止失败')
  } finally {
    killLoading.value = false
  }
}

async function loadHosts() {
  hostsLoading.value = true
  try {
    const res = await invoke<CommandResult<HostsEntry[]>>('tool_read_hosts')
    if (res.code === 0 && res.data) {
      hostsEntries.value = res.data
    } else {
      ElMessage.error(res.msg || '读取 hosts 文件失败')
    }
  } catch (e: any) {
    ElMessage.error(typeof e === 'string' ? e : (e?.message || '读取 hosts 文件失败'))
  } finally {
    hostsLoading.value = false
  }
}

async function saveHosts() {
  if (hostsEntries.value.length === 0) {
    ElMessage.warning('没有可保存的条目')
    return
  }
  const hasEmpty = hostsEntries.value.some(e => !e.ip.trim() || !e.host.trim())
  if (hasEmpty) {
    ElMessage.warning('存在空的 IP 或主机名，请检查')
    return
  }
  hostsSaving.value = true
  try {
    const entries = hostsEntries.value.map(e => ({ ip: e.ip, host: e.host, enabled: e.enabled }))
    const res = await invoke<CommandResult<string>>('tool_write_hosts', {
      params: { entries }
    })
    if (res.code === 0) {
      ElMessage.success(res.data || '保存成功')
      hostsEditing.value = false
      await loadHosts()
    } else {
      ElMessage.error(res.msg)
    }
  } catch (e) {
    ElMessage.error('保存失败')
  } finally {
    hostsSaving.value = false
  }
}

async function flushDns() {
  try {
    const res = await invoke<CommandResult<string>>('tool_flush_dns')
    if (res.code === 0) {
      ElMessage.success(res.data || 'DNS 缓存已刷新')
    } else {
      ElMessage.error(res.msg)
    }
  } catch (e) {
    ElMessage.error('刷新 DNS 失败')
  }
}

function addHostsEntry() {
  if (!hostsAddForm.value.ip.trim() || !hostsAddForm.value.host.trim()) {
    ElMessage.warning('请填写 IP 和主机名')
    return
  }
  hostsEditing.value = true
  hostsEntries.value.push({
    ip: hostsAddForm.value.ip.trim(),
    host: hostsAddForm.value.host.trim(),
    enabled: true,
    original_line: '',
    line_number: 0
  })
  hostsAddForm.value = { ip: '', host: '' }
}

function removeHostsEntry(index: number) {
  hostsEditing.value = true
  hostsEntries.value.splice(index, 1)
}

function toggleHostsEnabled(index: number) {
  hostsEditing.value = true
  hostsEntries.value[index].enabled = !hostsEntries.value[index].enabled
}

function cancelHostsEdit() {
  hostsEditing.value = false
  loadHosts()
}

function handleTabChange(tab: string) {
  if (tab === 'hosts' && hostsEntries.value.length === 0) {
    loadHosts()
  }
}

function handleClose() {
  emit('update:visible', false)
}
</script>

<template>
  <el-dialog
    :model-value="visible"
    title="系统工具箱"
    width="680px"
    @close="handleClose"
    destroy-on-close
    top="6vh"
  >
    <el-tabs v-model="activeTab" @tab-change="handleTabChange">
      <el-tab-pane label="查找进程" name="process">
        <div class="tool-row">
          <el-input
            v-model="processKeyword"
            placeholder="输入进程名关键词，如 node、java、nginx"
            clearable
            @keyup.enter="findProcess"
          >
            <template #prefix><el-icon><Search /></el-icon></template>
          </el-input>
          <el-button type="primary" :icon="Search" :loading="processLoading" @click="findProcess">搜索</el-button>
        </div>
        <el-table :data="processResults" max-height="360" stripe size="small" v-loading="processLoading" class="tool-table">
          <el-table-column prop="pid" label="PID" width="90" />
          <el-table-column prop="name" label="进程名" min-width="160" show-overflow-tooltip />
          <el-table-column prop="cpu_usage" label="CPU%" width="80" align="right">
            <template #default="{ row }">{{ row.cpu_usage.toFixed(2) }}%</template>
          </el-table-column>
          <el-table-column prop="memory_mb" label="内存" width="90" align="right">
            <template #default="{ row }">{{ row.memory_mb.toFixed(1) }}MB</template>
          </el-table-column>
          <el-table-column label="操作" width="80" align="center">
            <template #default="{ row }">
              <el-button type="danger" size="small" :icon="Delete" circle @click="handleKill(row.pid)" />
            </template>
          </el-table-column>
        </el-table>
      </el-tab-pane>

      <el-tab-pane label="查找端口" name="port">
        <div class="tool-row">
          <el-input-number
            v-model="portNumber"
            :min="1"
            :max="65535"
            placeholder="端口号"
            controls-position="right"
            style="width: 200px"
            @keyup.enter="findPort"
          />
          <el-button type="primary" :icon="Search" :loading="portLoading" @click="findPort">查找</el-button>
        </div>
        <el-table :data="portResults" max-height="360" stripe size="small" v-loading="portLoading" class="tool-table">
          <el-table-column prop="protocol" label="协议" width="70" />
          <el-table-column prop="local_addr" label="本地地址" min-width="180" />
          <el-table-column prop="state" label="状态" width="130" show-overflow-tooltip />
          <el-table-column prop="pid" label="PID" width="80" />
          <el-table-column prop="process_name" label="进程名" width="120" show-overflow-tooltip />
          <el-table-column label="操作" width="80" align="center">
            <template #default="{ row }">
              <el-button type="danger" size="small" :icon="Delete" circle @click="handleKill(row.pid)" />
            </template>
          </el-table-column>
        </el-table>
      </el-tab-pane>

      <el-tab-pane label="终止进程" name="kill">
        <div class="kill-section">
          <div class="tool-row">
            <el-input-number
              v-model="killPid"
              :min="1"
              placeholder="输入 PID"
              controls-position="right"
              style="width: 200px"
            />
            <el-button type="danger" :icon="Delete" :loading="killLoading" @click="handleKill()">终止进程</el-button>
          </div>
          <div class="kill-tip">
            <el-icon><Position /></el-icon>
            <span>输入进程 PID 强制终止，将同时终止该进程的所有子进程</span>
          </div>
        </div>
      </el-tab-pane>

      <el-tab-pane label="Hosts管理" name="hosts">
        <div class="hosts-actions">
          <div class="hosts-add-row">
            <el-input v-model="hostsAddForm.ip" placeholder="IP 地址，如 127.0.0.1" style="width: 180px" />
            <el-input v-model="hostsAddForm.host" placeholder="主机名，如 localhost" style="flex: 1" @keyup.enter="addHostsEntry" />
            <el-button type="primary" :icon="Plus" @click="addHostsEntry">添加</el-button>
          </div>
          <div class="hosts-toolbar">
            <el-button size="small" :icon="RefreshRight" :loading="hostsLoading" @click="loadHosts">刷新</el-button>
            <el-button size="small" @click="flushDns">刷新DNS缓存</el-button>
            <div style="flex: 1" />
            <el-button v-if="hostsEditing" size="small" @click="cancelHostsEdit">取消</el-button>
            <el-button v-if="hostsEditing" type="primary" size="small" :loading="hostsSaving" @click="saveHosts">保存</el-button>
          </div>
        </div>
        <el-table :data="hostsEntries" max-height="320" stripe size="small" v-loading="hostsLoading" class="tool-table">
          <el-table-column prop="ip" label="IP 地址" width="160">
            <template #default="{ row }">
              <span :class="{ 'hosts-disabled': !row.enabled }">{{ row.ip }}</span>
            </template>
          </el-table-column>
          <el-table-column prop="host" label="主机名" min-width="200">
            <template #default="{ row }">
              <span :class="{ 'hosts-disabled': !row.enabled }">{{ row.host }}</span>
            </template>
          </el-table-column>
          <el-table-column label="启用" width="70" align="center">
            <template #default="{ row, $index }">
              <el-switch
                size="small"
                :model-value="row.enabled"
                @change="toggleHostsEnabled($index)"
              />
            </template>
          </el-table-column>
          <el-table-column label="操作" width="70" align="center">
            <template #default="{ $index }">
              <el-button type="danger" size="small" :icon="Delete" circle @click="removeHostsEntry($index)" />
            </template>
          </el-table-column>
        </el-table>
        <div class="hosts-tip">
          <el-icon><Position /></el-icon>
          <span>修改 hosts 文件需要管理员权限运行本程序，修改后会自动备份原文件</span>
        </div>
      </el-tab-pane>
    </el-tabs>

    <template #footer>
      <el-button @click="handleClose">关闭</el-button>
    </template>
  </el-dialog>
</template>

<script lang="ts">
export default { name: 'ToolKit' }
</script>

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

:deep(.el-tabs__nav-wrap::after) {
  height: 1px;
}

.tool-row {
  display: flex;
  gap: 10px;
  margin-bottom: 14px;

  .el-input, .el-input-number {
    flex: 1;
  }
}

.tool-table {
  width: 100%;
  border-radius: 8px;
  overflow: hidden;
}

.kill-section {
  .kill-tip {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 12px;
    padding: 10px 14px;
    background: var(--el-color-warning-light-9);
    border-radius: 8px;
    font-size: 12.5px;
    color: var(--el-text-color-secondary);
  }
}

.hosts-actions {
  margin-bottom: 14px;

  .hosts-add-row {
    display: flex;
    gap: 10px;
    margin-bottom: 10px;
  }

  .hosts-toolbar {
    display: flex;
    gap: 8px;
    align-items: center;
  }
}

.hosts-disabled {
  color: var(--el-text-color-placeholder);
  text-decoration: line-through;
}

.hosts-tip {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 12px;
  padding: 10px 14px;
  background: var(--el-color-warning-light-9);
  border-radius: 8px;
  font-size: 12.5px;
  color: var(--el-text-color-secondary);
}
</style>
