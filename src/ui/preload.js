// 桥接适配层：把原 Electron 的 window.buddy 契约映射到 Tauri 2 IPC/事件。
// 渲染器（renderer.js）不感知运行时差异；原窗口收起细节面板的行为对应窗口失焦。
(function () {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const listeners = { state: [], dismiss: [] };
  listen('core-status', event => listeners.state.forEach(cb => cb(event.payload))).catch(console.error);
  listen('core-failed', event => {
    const payload = event.payload || {};
    listeners.state.forEach(cb => cb({ phase: 'error', message: payload.message || '核心服务启动失败', models: [], modelResults: {}, availableModels: [] }));
  }).catch(console.error);
  listen('tauri://blur', () => listeners.dismiss.forEach(cb => cb())).catch(console.error);
  window.buddy = {
    async action(name, value) {
      try {
        if (name === 'restart') { await invoke('restart_core'); return { ok: true, result: {} }; }
        if (name === 'import' && !value) {
          const selected = await window.__TAURI__.dialog.open({
            multiple: false, directory: false,
            filters: [{ name: 'WorkBuddy models.json', extensions: ['json'] }],
          });
          if (!selected) return { ok: true, result: { canceled: true } };
          value = { modelsFile: selected };
        }
        const result = await invoke('core_action', { action: name, payload: value ?? null });
        return { ok: true, result };
      } catch (error) {
        return { ok: false, error: String(error) };
      }
    },
    onState(cb) { listeners.state.push(cb); },
    onDismiss(cb) { listeners.dismiss.push(cb); },
  };
})();
