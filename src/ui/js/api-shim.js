/*
 * api-shim.js —— Tauri 版前端桥接层
 * 与 Electron 版 preload.js 的 contextBridge 契约逐条对齐:
 *   - 方法名/参数/返回值形状完全一致, app.js 与 pages.js 零改动
 *   - 命令名映射: camelCase -> Rust snake_case (如 cleanScan -> clean_scan)
 *   - 事件频道白名单与 preload.js EVENT_CHANNELS 相同
 */
(function () {
  'use strict';

  var EVENT_CHANNELS = new Set([
    'speedup:progress',
    'speedup:item-status',
    'speedup:done',
    'clean:progress',
    'clean:group-status',
    'clean:done',
    'clean:exec-progress',
    'shredder:progress',
  ]);

  var T = window.__TAURI__;
  if (!T || !T.core || !T.core.invoke) {
    console.error('[api-shim] __TAURI__ global missing — 检查 tauri.conf.json withGlobalTauri');
  }

  function invoke(cmd, args) {
    return T.core.invoke(cmd, args || {});
  }

  var api = {
    // ---- App / Store ----
    getAppStats: function () { return invoke('get_app_stats'); },
    storeGet: function (key) { return invoke('store_get', { key: key }); },
    storeSet: function (key, value) { return invoke('store_set', { key: key, value: value }); },

    // ---- 加速 ----
    speedupScan: function () { return invoke('speedup_scan'); },
    speedupCancel: function () { return invoke('speedup_cancel'); },
    speedupOptimize: function (fixIds) { return invoke('speedup_optimize', { fixIds: fixIds }); },

    // ---- 清理 ----
    cleanScan: function (tab) { return invoke('clean_scan', { tab: tab }); },
    cleanCancel: function () { return invoke('clean_cancel'); },
    cleanExecute: function (items) { return invoke('clean_execute', { items: items }); },
    cleanItemFiles: function (groupId, itemId) {
      return invoke('clean_item_files', { groupId: groupId, itemId: itemId });
    },

    // ---- 文件夹/资源管理器 ----
    openFolder: function (filePath) { return invoke('open_folder', { filePath: filePath }); },

    // ---- 碎纸机 ----
    shredderOpenFile: function () { return invoke('shredder_open_file'); },
    shredderOpenFolder: function () { return invoke('shredder_open_folder'); },
    shredderBrowseFolder: function (folderPath) {
      return invoke('shredder_browse_folder', { folderPath: folderPath });
    },
    shredderStatFile: function (filePath) {
      return invoke('shredder_stat_file', { filePath: filePath });
    },
    shredFile: function (filePath, method) {
      return invoke('shred_file', { filePath: filePath, method: method });
    },
    shredFolder: function (folderPath, method) {
      return invoke('shred_folder', { folderPath: folderPath, method: method });
    },
    shredCancel: function () { return invoke('shred_cancel'); },

    // ---- 启动项 ----
    startupList: function (tab) { return invoke('startup_list', { tab: tab }); },
    startupToggle: function (itemId, enabled) {
      return invoke('startup_toggle', { itemId: itemId, enabled: enabled });
    },
    startupRemove: function (itemId) { return invoke('startup_remove', { itemId: itemId }); },
    startupOpenLocation: function (itemId) {
      return invoke('startup_open_location', { itemId: itemId });
    },
    startupDetail: function (itemId) { return invoke('startup_detail', { itemId: itemId }); },
    startupSmartOptimize: function () { return invoke('startup_smart_optimize'); },
    startupAdd: function (item) { return invoke('startup_add', { item: item }); },
    startupBackup: function () { return invoke('startup_backup'); },
    startupRestore: function (fileName) {
      return invoke('startup_restore', { fileName: fileName });
    },
    startupListBackups: function () { return invoke('startup_list_backups'); },
    startupSetIgnored: function (itemId, ignored) {
      return invoke('startup_set_ignored', { itemId: itemId, ignored: ignored });
    },

    // ---- 图标 ----
    getFileIcon: function (filePath) { return invoke('get_file_icon', { filePath: filePath }); },

    // ---- 外部链接: 用系统默认浏览器打开 ----
    openUrl: function (url) {
      try {
        if (T && T.opener && T.opener.openUrl) { return T.opener.openUrl(url); }
      } catch (e) {}
      try { window.open(url, '_blank'); } catch (e) {}
      return Promise.resolve();
    },

    // ---- 设置 ----
    settingsGetAutostart: function () { return invoke('settings_get_autostart'); },
    settingsSetAutostart: function (enabled) {
      return invoke('settings_set_autostart', { enabled: enabled });
    },


    // ---- 事件订阅 (白名单外一律拒绝, 行为同 preload.js) ----
    on: function (channel, cb) {
      if (!EVENT_CHANNELS.has(channel)) {
        console.warn('[api-shim] blocked subscription to non-whitelisted channel:', channel);
        return function () {};
      }
      var p = T.event.listen(channel, function (ev) { cb(ev.payload); });
      return function () { p.then(function (un) { un(); }).catch(function () {}); };
    },
  };

  window.api = api;

  // electronAPI: 无边框窗口的自定义标题栏按钮
  window.electronAPI = {
    minimize: function () { return invoke('window_minimize'); },
    close: function () { return invoke('window_close'); },
  };

  // 前端就绪后淡出启动加载层 (双rAF确保应用首帧已绘制)
  function dismissBootLoading() {
    var el = document.getElementById('boot-loading');
    if (!el) return;
    requestAnimationFrame(function () {
      requestAnimationFrame(function () {
        el.classList.add('hide');
        setTimeout(function () { if (el.parentNode) el.parentNode.removeChild(el); }, 300);
      });
    });
  }
  if (document.readyState === 'complete') dismissBootLoading();
  else window.addEventListener('load', dismissBootLoading, { once: true });

  console.log('[api-shim] ready (Tauri bridge)');
})();
