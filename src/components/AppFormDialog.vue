<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { ElMessage } from 'element-plus'
import { Plus, Delete, FolderOpened } from '@element-plus/icons-vue'
import type { AppInstance, AppConfig, AppType, CommandResult, ProxyRule } from '../App'

const props = defineProps<{
  visible: boolean
  editingApp: AppInstance | null
  copySource: AppInstance | null
  groups?: string[]
}>()

const emit = defineEmits<{
  'update:visible': [val: boolean]
  success: []
}>()

const PRESET_COLORS = [
  '#409EFF', '#67C23A', '#E6A23C', '#F56C6C',
  '#909399', '#9B59B6', '#1ABC9C', '#E74C3C',
  '#3498DB', '#2ECC71', '#F39C12', '#E91E63',
]

const appType = ref<AppType>('Command')

const form = ref({
  name: '',
  command: '',
  work_dir: '',
  description: '',
  color: '',
  auto_start: false,
  group: '',
  env_vars: [] as { key: string; value: string }[],
  delay_seconds: 0,
  static_port: 8080,
  static_root_dir: '',
  static_spa: true,
  static_index: 'index.html',
  proxy_rules: [] as ProxyRule[],
  url: '',
  watch_restart: false,
  watch_dirs: [] as string[],
  exit_restart: false
})

const submitting = ref(false)
const isEdit = ref(false)
const dialogTitle = computed(() => isEdit.value ? '编辑应用' : '添加应用')

function fillForm(cfg: AppConfig, nameSuffix = '') {
  const envVars = cfg.env_vars
    ? Object.entries(cfg.env_vars).map(([key, value]) => ({ key, value }))
    : []
  appType.value = cfg.app_type || 'Command'
  form.value = {
    name: nameSuffix ? `${cfg.name}${nameSuffix}` : cfg.name,
    command: cfg.command,
    work_dir: cfg.work_dir || '',
    description: cfg.description || '',
    color: cfg.color || '',
    auto_start: cfg.auto_start,
    group: cfg.group || '',
    env_vars: envVars,
    delay_seconds: cfg.delay_seconds,
    static_port: cfg.static_server?.port || 8080,
    static_root_dir: cfg.static_server?.root_dir || '',
    static_spa: cfg.static_server?.spa_mode ?? true,
    static_index: cfg.static_server?.index_file || 'index.html',
    proxy_rules: cfg.static_server?.proxy_rules
      ? [...cfg.static_server.proxy_rules]
      : [],
    url: cfg.url || '',
    watch_restart: cfg.watch_restart || false,
    watch_dirs: cfg.watch_dirs ? [...cfg.watch_dirs] : [],
    exit_restart: cfg.exit_restart || false
  }
}

watch(() => props.visible, (val) => {
  if (!val) return
  if (props.editingApp) {
    isEdit.value = true
    fillForm(props.editingApp.config)
  } else if (props.copySource) {
    // 复制为新增：预填被复制应用的配置，但按“添加应用”提交（后端生成新 ID）
    isEdit.value = false
    fillForm(props.copySource.config, ' 副本')
  } else {
    isEdit.value = false
    appType.value = 'Command'
    form.value = {
      name: '',
      command: '',
      work_dir: '',
      description: '',
      color: '',
      auto_start: false,
      group: '',
      env_vars: [],
      delay_seconds: 0,
      static_port: 8080,
      static_root_dir: '',
      static_spa: true,
      static_index: 'index.html',
      proxy_rules: [],
      url: '',
      watch_restart: false,
      watch_dirs: [] as string[],
      exit_restart: false
    }
  }
})

const formRules = computed(() => ({
  name: [{ required: true, message: '请输入应用名称', trigger: 'blur' }],
  command: [{
    required: appType.value === 'Command',
    message: '请输入启动命令',
    trigger: 'blur'
  }],
  static_root_dir: [{
    required: appType.value === 'StaticServer',
    message: '请输入静态文件目录',
    trigger: 'blur'
  }]
}))

const formRef = ref()

function addEnvVar() {
  form.value.env_vars.push({ key: '', value: '' })
}

function removeEnvVar(index: number) {
  form.value.env_vars.splice(index, 1)
}

function addProxyRule() {
  form.value.proxy_rules.push({ path: '/api', target: 'http://localhost:8080', rewrite: true })
}

function removeProxyRule(index: number) {
  form.value.proxy_rules.splice(index, 1)
}

function addWatchDir() {
  form.value.watch_dirs.push('')
}

function removeWatchDir(index: number) {
  form.value.watch_dirs.splice(index, 1)
}

async function selectWatchDir(index: number) {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: '选择监控目录'
    })
    if (selected) {
      form.value.watch_dirs[index] = selected
    }
  } catch (e) {
    console.error('选择文件夹失败', e)
  }
}

async function selectFolder() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: '选择静态文件目录'
    })
    if (selected) {
      form.value.static_root_dir = selected
    }
  } catch (e) {
    console.error('选择文件夹失败', e)
  }
}

async function selectWorkDir() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: '选择工作目录'
    })
    if (selected) {
      form.value.work_dir = selected
    }
  } catch (e) {
    console.error('选择文件夹失败', e)
  }
}

function buildEnvVars(): Record<string, string> | null {
  const valid = form.value.env_vars.filter(e => e.key.trim() !== '')
  if (valid.length === 0) return null
  const obj: Record<string, string> = {}
  for (const e of valid) {
    obj[e.key.trim()] = e.value
  }
  return obj
}

function buildStaticServer() {
  if (appType.value !== 'StaticServer') return null
  return {
    port: form.value.static_port,
    root_dir: form.value.static_root_dir,
    spa_mode: form.value.static_spa,
    index_file: form.value.static_index,
    proxy_rules: form.value.proxy_rules.filter(r => r.path && r.target)
  }
}

async function handleSubmit() {
  try {
    await formRef.value?.validate()
  } catch {
    return
  }

  submitting.value = true
  try {
    const payload = {
      name: form.value.name,
      app_type: appType.value,
      command: appType.value === 'Command' ? form.value.command : '',
      work_dir: form.value.work_dir || null,
      description: form.value.description || null,
      color: form.value.color || null,
      auto_start: form.value.auto_start,
      group: form.value.group || null,
      env_vars: appType.value === 'Command' ? buildEnvVars() : null,
      delay_seconds: form.value.delay_seconds,
      static_server: buildStaticServer(),
      url: appType.value === 'Command' ? (form.value.url || null) : null,
      watch_restart: form.value.watch_restart,
      watch_dirs: form.value.watch_dirs.filter(d => d.trim() !== '').length > 0
        ? form.value.watch_dirs.filter(d => d.trim() !== '')
        : null,
      exit_restart: form.value.exit_restart
    }

    if (isEdit.value && props.editingApp) {
      const res = await invoke<CommandResult<null>>('update_app', {
        params: { id: props.editingApp.config.id, ...payload }
      })
      if (res.code === 0) {
        ElMessage.success('更新成功')
        emit('success')
      } else {
        ElMessage.error(res.msg)
      }
    } else {
      const res = await invoke<CommandResult<null>>('add_app', {
        params: payload
      })
      if (res.code === 0) {
        ElMessage.success('添加成功')
        emit('success')
      } else {
        ElMessage.error(res.msg)
      }
    }
  } catch (e: any) {
    console.error('表单提交失败:', e)
    ElMessage.error(typeof e === 'string' ? e : (e?.message || '操作失败'))
  } finally {
    submitting.value = false
  }
}

function handleClose() {
  emit('update:visible', false)
}
</script>

<template>
  <el-dialog
    :model-value="visible"
    :title="dialogTitle"
    width="620px"
    @close="handleClose"
    destroy-on-close
  >
    <el-form
      ref="formRef"
      :model="form"
      :rules="formRules"
      label-width="90px"
      label-position="right"
    >
      <el-form-item label="应用名称" prop="name">
        <el-input v-model="form.name" placeholder="例如：Nginx Web Server" />
      </el-form-item>

      <el-form-item label="应用类型">
        <el-radio-group v-model="appType" :disabled="isEdit">
          <el-radio-button value="Command">命令模式</el-radio-button>
          <el-radio-button value="StaticServer">静态服务器</el-radio-button>
        </el-radio-group>
        <div class="type-hint">
          <template v-if="appType === 'Command'">通过启动命令运行外部程序或服务</template>
          <template v-else>内置静态文件服务器，适合托管前端项目和静态页面</template>
        </div>
      </el-form-item>

      <template v-if="appType === 'Command'">
        <el-form-item label="启动命令" prop="command">
          <el-input
            v-model="form.command"
            placeholder="例如：nginx -g 'daemon off;'"
            type="textarea"
            :rows="3"
          />
        </el-form-item>

        <el-form-item label="工作目录">
          <div class="dir-input-row">
            <el-input v-model="form.work_dir" placeholder="可选，进程的工作目录" />
            <el-button :icon="FolderOpened" @click="selectWorkDir" plain>选择</el-button>
          </div>
        </el-form-item>

        <el-form-item label="环境变量">
          <div class="env-vars-editor">
            <div v-for="(env, idx) in form.env_vars" :key="idx" class="env-row">
              <el-input v-model="env.key" placeholder="变量名" style="width: 40%" />
              <span class="env-eq">=</span>
              <el-input v-model="env.value" placeholder="变量值" style="width: 45%" />
              <el-button :icon="Delete" circle size="small" @click="removeEnvVar(idx)" />
            </div>
            <el-button size="small" :icon="Plus" @click="addEnvVar" plain>添加变量</el-button>
          </div>
        </el-form-item>

        <el-form-item label="访问地址">
          <el-input v-model="form.url" placeholder="可选，例如：http://localhost:3000（用于快速打开浏览器）" />
        </el-form-item>
      </template>

      <template v-else>
        <el-form-item label="端口" prop="static_port">
          <el-input-number
            v-model="form.static_port"
            :min="1"
            :max="65535"
            controls-position="right"
            style="width: 180px"
          />
          <span class="delay-hint">访问地址: http://localhost:{{ form.static_port }}</span>
        </el-form-item>

        <el-form-item label="文件目录" prop="static_root_dir">
          <div class="dir-input-row">
            <el-input
              v-model="form.static_root_dir"
              placeholder="静态文件根目录，例如：D:/projects/my-app/dist"
            />
            <el-button :icon="FolderOpened" @click="selectFolder" plain>选择</el-button>
          </div>
        </el-form-item>

        <el-form-item label="SPA 模式">
          <el-switch
            v-model="form.static_spa"
            active-text="启用"
            inactive-text="关闭"
          />
          <span class="delay-hint">路由未匹配时返回 index.html（Vue/React 等单页应用需要开启）</span>
        </el-form-item>

        <el-form-item label="默认首页">
          <el-input v-model="form.static_index" placeholder="index.html" style="width: 200px" />
        </el-form-item>

        <el-divider content-position="left">反向代理规则</el-divider>
        <div class="proxy-hint">
          类似 Vite 的 proxy 配置，将指定路径的请求转发到后端服务
        </div>

        <div class="proxy-rules-editor">
          <div v-for="(rule, idx) in form.proxy_rules" :key="idx" class="proxy-row">
            <el-input v-model="rule.path" placeholder="/api" style="width: 25%">
              <template #prepend>路径</template>
            </el-input>
            <el-input v-model="rule.target" placeholder="http://localhost:8080" style="width: 40%">
              <template #prepend>目标</template>
            </el-input>
            <el-checkbox v-model="rule.rewrite" label="重写路径" />
            <el-button :icon="Delete" circle size="small" type="danger" @click="removeProxyRule(idx)" />
          </div>
          <el-button size="small" :icon="Plus" @click="addProxyRule" plain>添加代理规则</el-button>
        </div>
      </template>

      <el-divider content-position="left">通用设置</el-divider>

      <el-form-item label="分组">
        <el-select
          v-model="form.group"
          placeholder="可选，例如：Web服务、数据库"
          filterable
          allow-create
          default-first-option
          clearable
          style="width: 100%"
        >
          <el-option v-for="g in props.groups" :key="g" :label="g" :value="g" />
        </el-select>
      </el-form-item>

      <el-form-item label="描述">
        <el-input
          v-model="form.description"
          placeholder="可选，应用的简要描述"
          type="textarea"
          :rows="2"
        />
      </el-form-item>

      <el-form-item label="标签颜色">
        <div class="color-picker">
          <div
            v-for="c in PRESET_COLORS"
            :key="c"
            class="color-item"
            :class="{ active: form.color === c }"
            :style="{ backgroundColor: c }"
            @click="form.color = form.color === c ? '' : c"
          />
          <div
            class="color-item none-color"
            :class="{ active: !form.color }"
            @click="form.color = ''"
          >
            无
          </div>
        </div>
      </el-form-item>

      <el-form-item label="启动延迟">
        <el-input-number
          v-model="form.delay_seconds"
          :min="0"
          :max="300"
          :step="1"
          controls-position="right"
        />
        <span class="delay-hint">秒（批量启动时的延迟）</span>
      </el-form-item>

      <el-form-item label="自动启动">
        <el-switch
          v-model="form.auto_start"
          active-text="随管理器启动"
          inactive-text="手动启动"
        />
      </el-form-item>

      <el-form-item label="退出重启" v-if="appType === 'Command'">
        <el-switch
          v-model="form.exit_restart"
          active-text="进程退出自动重启"
          inactive-text="关闭"
        />
        <span class="delay-hint">启用后，进程意外退出时将自动重新启动（手动停止不会触发）</span>
      </el-form-item>

      <el-form-item label="文件监控">
        <el-switch
          v-model="form.watch_restart"
          active-text="文件改动自动重启"
          inactive-text="关闭"
        />
        <span class="delay-hint">启用后，监控目录内文件变动将自动重启应用</span>
      </el-form-item>

      <el-form-item label="监控目录" v-if="form.watch_restart">
        <div class="watch-dirs-editor">
          <div class="watch-hint" v-if="form.watch_dirs.length === 0">
            未指定目录时，将使用工作目录作为监控目录
          </div>
          <div v-for="(_dir, idx) in form.watch_dirs" :key="idx" class="watch-dir-row">
            <el-input v-model="form.watch_dirs[idx]" placeholder="监控目录路径" style="flex: 1" />
            <el-button :icon="FolderOpened" @click="selectWatchDir(idx)" plain>选择</el-button>
            <el-button :icon="Delete" circle size="small" @click="removeWatchDir(idx)" />
          </div>
          <el-button size="small" :icon="Plus" @click="addWatchDir" plain>添加监控目录</el-button>
        </div>
      </el-form-item>
    </el-form>

    <template #footer>
      <el-button @click="handleClose">取消</el-button>
      <el-button type="primary" @click="handleSubmit" :loading="submitting">
        {{ isEdit ? '保存' : '添加' }}
      </el-button>
    </template>
  </el-dialog>
</template>

<style scoped lang="scss">
:deep(.el-dialog__header) {
  padding: 20px 24px 16px;
  border-bottom: 1px solid var(--el-border-color-extra-light);
  margin-right: 0;
}

:deep(.el-dialog__body) {
  padding: 20px 24px;
  max-height: 65vh;
  overflow-y: auto;
}

:deep(.el-dialog__footer) {
  padding: 16px 24px;
  border-top: 1px solid var(--el-border-color-extra-light);
}

:deep(.el-form-item) {
  margin-bottom: 18px;
}

:deep(.el-form-item__label) {
  font-weight: 500;
  font-size: 13px;
}

:deep(.el-divider__text) {
  font-size: 13px;
  font-weight: 600;
  color: var(--el-text-color-primary);
}

:deep(.el-radio-button__inner) {
  font-size: 13px;
}

.type-hint {
  font-size: 12px;
  color: var(--el-text-color-placeholder);
  margin-top: 6px;
  line-height: 1.5;
}

.dir-input-row {
  display: flex;
  gap: 8px;
  width: 100%;

  .el-input { flex: 1; }
}

.color-picker {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  align-items: center;

  .color-item {
    width: 30px;
    height: 30px;
    border-radius: 8px;
    cursor: pointer;
    border: 2px solid transparent;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    justify-content: center;

    &:hover { transform: scale(1.15); }

    &.active {
      border-color: var(--el-text-color-primary);
      box-shadow: 0 0 0 3px rgba(64, 158, 255, 0.15);
    }

    &.none-color {
      background: var(--el-fill-color-light);
      border: 2px dashed var(--el-border-color);
      font-size: 10px;
      color: var(--el-text-color-placeholder);
      font-weight: 500;
      &.active { border-color: var(--el-text-color-primary); border-style: solid; }
    }
  }
}

.env-vars-editor {
  width: 100%;

  .env-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    padding: 6px 8px;
    background: var(--el-fill-color-extra-light);
    border-radius: 8px;
  }

  .env-eq {
    color: var(--el-text-color-placeholder);
    font-weight: 700;
    font-family: 'SF Mono', 'Consolas', monospace;
  }
}

.proxy-hint {
  font-size: 12px;
  color: var(--el-text-color-placeholder);
  margin-bottom: 12px;
  line-height: 1.5;
}

.proxy-rules-editor {
  width: 100%;
  margin-bottom: 16px;

  .proxy-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
    padding: 8px;
    background: var(--el-fill-color-extra-light);
    border-radius: 8px;
  }
}

.delay-hint {
  margin-left: 10px;
  font-size: 12px;
  color: var(--el-text-color-placeholder);
}

.watch-dirs-editor {
  width: 100%;

  .watch-hint {
    font-size: 12px;
    color: var(--el-text-color-placeholder);
    margin-bottom: 8px;
    line-height: 1.5;
  }

  .watch-dir-row {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 8px;
    padding: 6px 8px;
    background: var(--el-fill-color-extra-light);
    border-radius: 8px;
  }
}
</style>
