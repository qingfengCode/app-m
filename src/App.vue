<script setup lang="ts">
import AppManager from './index.vue'
</script>

<template>
  <AppManager />
</template>

<style>
@import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap');

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html, body, #app {
  height: 100%;
  width: 100%;
  overflow: hidden;
}

body {
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  background: linear-gradient(135deg, #f5f6f8 0%, #eceef2 100%);
  color: #1d2129;
  transition: background 0.4s ease, color 0.3s ease;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

html.dark body {
  background: linear-gradient(135deg, #0a0a0b 0%, #111113 100%);
  color: #e5eaf3;
}

html.dark .el-card {
  --el-card-bg-color: #1a1a1c;
  border-color: rgba(255, 255, 255, 0.06);
  background: #1a1a1c;
}

html.dark .el-empty__description p {
  color: #6b7280;
}

html.dark .el-dialog {
  --el-dialog-bg-color: #1a1a1c;
}

::-webkit-scrollbar {
  width: 4px;
  height: 4px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: rgba(144, 147, 153, 0.25);
  border-radius: 10px;
}

::-webkit-scrollbar-thumb:hover {
  background: rgba(144, 147, 153, 0.4);
}

.el-button {
  border-radius: 6px !important;
  font-weight: 500;
}

.el-card {
  border-radius: 10px !important;
  border: 1px solid rgba(0, 0, 0, 0.04);
}

html.dark .el-card {
  border: 1px solid rgba(255, 255, 255, 0.06);
}

.el-tag {
  border-radius: 5px !important;
}

.el-input__wrapper {
  border-radius: 6px !important;
}

.el-radio-button__inner {
  border-radius: 0 !important;
}

.el-radio-button:first-child .el-radio-button__inner {
  border-radius: 6px 0 0 6px !important;
}

.el-radio-button:last-child .el-radio-button__inner {
  border-radius: 0 6px 6px 0 !important;
}

.el-dialog {
  border-radius: 12px !important;
  overflow: hidden;
}

.el-select__wrapper {
  border-radius: 6px !important;
}

.el-progress-bar__outer {
  border-radius: 4px !important;
}

.el-progress-bar__inner {
  border-radius: 4px !important;
}
</style>
