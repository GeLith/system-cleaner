/**
 * app.js - View router, sidebar state, window controls, event helpers
 */
(function () {
  'use strict';

  /* -----------------------------------------------------------
     IPC Guard
     ----------------------------------------------------------- */
  const apiAvailable = typeof window !== 'undefined' && window.api;
  if (!apiAvailable) {
    window.api = createFallbackApi();
    var fallbackEl = document.getElementById('apiFallback');
    if (fallbackEl) {
      fallbackEl.style.display = '';
      setTimeout(function () {
        fallbackEl.style.display = 'none';
      }, 4000);
    }
  }

  function createFallbackApi() {
    var noop = function () {};
    var noResolve = function () { return Promise.resolve(null); };
    return {
      getAppStats: noResolve,
      speedupScan: noop,
      speedupCancel: noop,
      speedupOptimize: function () { return Promise.resolve({ ok: false }); },
      cleanScan: noop,
      cleanCancel: noop,
      cleanExecute: function () { return Promise.resolve({ ok: false }); },
      startupList: function () { return Promise.resolve([]); },
      startupToggle: function () { return Promise.resolve({ ok: false }); },
      startupSmartOptimize: function () { return Promise.resolve({ ok: false, count: 0 }); },
      storeGet: function () { return Promise.resolve(null); },
      storeSet: function () { return Promise.resolve(); },
      on: noop,
    };
  }

  /* -----------------------------------------------------------
     Event Subscription Manager
     ----------------------------------------------------------- */
  var _listeners = {};

  /**
   * Subscribe to an IPC event channel. Returns an unsubscribe function.
   * Wraps window.api.on to allow multiple subscribers per channel.
   */
  function subscribe(channel, cb) {
    if (!_listeners[channel]) {
      _listeners[channel] = [];
      window.api.on(channel, function (payload) {
        var cbs = _listeners[channel];
        for (var i = 0; i < cbs.length; i++) {
          try { cbs[i](payload); } catch (e) { console.error('[Event error]', channel, e); }
        }
      });
    }
    _listeners[channel].push(cb);
    return function unsubscribe() {
      var arr = _listeners[channel];
      if (!arr) return;
      var idx = arr.indexOf(cb);
      if (idx !== -1) arr.splice(idx, 1);
    };
  }

  /* -----------------------------------------------------------
     View Router
     ----------------------------------------------------------- */
  var currentView = 'welcome';
  var views = ['welcome', 'speedup', 'clean', 'startup', 'shredder', 'settings'];

  function switchView(name) {
    if (views.indexOf(name) === -1) return;
    var oldIdx = views.indexOf(currentView);
    var newIdx = views.indexOf(name);
    var dir = newIdx <= oldIdx ? 'slide-from-left' : 'slide-from-right';
    // Update sidebar active state
    var items = document.querySelectorAll('.sidebar-item');
    for (var i = 0; i < items.length; i++) {
      items[i].classList.toggle('active', items[i].getAttribute('data-view') === name);
    }
    // Update view visibility
    var viewEls = document.querySelectorAll('.view');
    for (var j = 0; j < viewEls.length; j++) {
      var el = viewEls[j];
      var isActive = el.id === 'view-' + name;
      el.classList.remove('slide-from-left', 'slide-from-right');
      el.classList.toggle('active', isActive);
      // apply directional entrance only when switching (not initial load)
      if (isActive && currentView !== name) {
        el.classList.add(dir);
        el.addEventListener('animationend', function handler() {
          el.classList.remove('slide-from-left', 'slide-from-right');
          el.removeEventListener('animationend', handler);
        }, { once: true });
      }
    }
    currentView = name;
    // Fire view change event for pages.js
    document.dispatchEvent(new CustomEvent('viewchange', { detail: { view: name } }));
  }

  /* -----------------------------------------------------------
     Sidebar Toggle
     ----------------------------------------------------------- */
  var sidebar = document.getElementById('sidebar');
  var sidebarToggle = document.getElementById('sidebarToggle');

  function setSidebarState(expanded) {
    sidebar.className = 'sidebar ' + (expanded ? 'expanded' : 'collapsed');
    document.body.classList.toggle('sidebar-collapsed', !expanded);
    if (sidebarToggle) {
      sidebarToggle.setAttribute('aria-label', expanded ? '收起侧边栏' : '展开侧边栏');
      sidebarToggle.setAttribute('title', expanded ? '收起侧边栏' : '展开侧边栏');
    }
    try { window.api.storeSet('sidebarExpanded', expanded); } catch (e) {}
    try { localStorage.setItem('sidebarExpanded', expanded ? '1' : '0'); } catch (e) {}
  }

  function initSidebar() {
    var setFromStored = function (stored) {
      var expanded = stored === null || stored === undefined ? true : (stored === '1' || stored === true);
      setSidebarState(expanded);
    };
    try {
      Promise.resolve(window.api.storeGet('sidebarExpanded')).then(function (stored) {
        if (stored === null || stored === undefined) {
          try { stored = localStorage.getItem('sidebarExpanded'); } catch (e) {}
        }
        setFromStored(stored);
      }).catch(function () {
        var local = null;
        try { local = localStorage.getItem('sidebarExpanded'); } catch (e) {}
        setFromStored(local);
      });
    } catch (e) {
      setFromStored(null);
    }
  }

  sidebarToggle.addEventListener('click', function () {
    var isExpanded = sidebar.classList.contains('expanded');
    setSidebarState(!isExpanded);
  });

  /* -----------------------------------------------------------
     Sidebar Navigation
     ----------------------------------------------------------- */
  document.querySelectorAll('.sidebar-item a').forEach(function (link) {
    link.addEventListener('click', function (e) {
      e.preventDefault();
      var view = this.getAttribute('data-view');
      switchView(view);
    });
  });

  /* -----------------------------------------------------------
     Sidebar Keyboard Navigation
     ----------------------------------------------------------- */
  (function () {
    var navLinks = document.querySelectorAll('.sidebar-nav .sidebar-item a');
    var navArray = Array.prototype.slice.call(navLinks);
    var sidebarNav = document.querySelector('.sidebar-nav');
    if (!sidebarNav) return;

    sidebarNav.addEventListener('keydown', function (e) {
      var focused = document.activeElement;
      var idx = navArray.indexOf(focused);
      if (idx === -1) return;

      var handled = false;
      switch (e.key) {
        case 'ArrowDown':
          idx = (idx + 1) % navArray.length;
          navArray[idx].focus();
          handled = true;
          break;
        case 'ArrowUp':
          idx = (idx - 1 + navArray.length) % navArray.length;
          navArray[idx].focus();
          handled = true;
          break;
        case 'Enter':
        case ' ':
          e.preventDefault();
          var view = focused.getAttribute('data-view');
          if (view) switchView(view);
          handled = true;
          break;
        case 'Home':
          navArray[0].focus();
          handled = true;
          break;
        case 'End':
          navArray[navArray.length - 1].focus();
          handled = true;
          break;
      }
      if (handled) e.preventDefault();
    });
  })();

  /* -----------------------------------------------------------
     Sidebar Collapsed Tooltips
     ----------------------------------------------------------- */
  (function () {
    var tooltip = document.createElement('div');
    tooltip.className = 'sidebar-tooltip';
    document.body.appendChild(tooltip);

    var navLinks = document.querySelectorAll('.sidebar-nav .sidebar-item a');

    function showTooltip(el) {
      var label = el.getAttribute('data-label');
      if (!label) return;
      tooltip.textContent = label;
      tooltip.classList.add('visible');
      // Force reflow so offsetHeight is accurate after text change
      void tooltip.offsetHeight;
      var rect = el.getBoundingClientRect();
      var tipH = tooltip.offsetHeight;
      tooltip.style.left = Math.round(rect.right + 8) + 'px';
      tooltip.style.top = Math.round(rect.top + (rect.height - tipH) / 2) + 'px';
    }

    function hideTooltip() {
      tooltip.classList.remove('visible');
    }

    navLinks.forEach(function (link) {
      link.addEventListener('mouseenter', function () {
        if (sidebar.classList.contains('collapsed')) {
          showTooltip(this);
        }
      });
      link.addEventListener('mouseleave', hideTooltip);
      link.addEventListener('focus', function () {
        if (sidebar.classList.contains('collapsed')) {
          showTooltip(this);
        }
      });
      link.addEventListener('blur', hideTooltip);
    });

    // Hide tooltip when sidebar state changes
    sidebarToggle.addEventListener('click', function () {
      hideTooltip();
    });
  })();

  /* -----------------------------------------------------------
     Welcome Guide Links
     ----------------------------------------------------------- */
  document.querySelectorAll('.guide-item').forEach(function (el) {
    el.style.cursor = 'pointer';
    el.addEventListener('click', function () {
      switchView(this.getAttribute('data-view'));
    });
  });

  /* -----------------------------------------------------------
     Window Controls
     ----------------------------------------------------------- */
  document.getElementById('btnMinimize').addEventListener('click', function () {
    if (window.electronAPI && window.electronAPI.minimize) {
      window.electronAPI.minimize();
    }
    // If preload exposes it differently, try ipcRenderer
    try {
      var ipc = window.electron && window.electron.ipcRenderer;
      if (ipc) ipc.send('window:minimize');
    } catch (e) {}
  });

  document.getElementById('btnClose').addEventListener('click', function () {
    function doClose() {
      if (window.electronAPI && window.electronAPI.close) {
        window.electronAPI.close();
      }
      try {
        var ipc = window.electron && window.electron.ipcRenderer;
        if (ipc) ipc.send('window:close');
      } catch (e) {}
    }
    Promise.resolve(window.api.storeGet('confirmClose')).then(function (v) {
      if (v) {
        if (window.confirm('确定要退出系统清理工具吗？')) doClose();
      } else {
        doClose();
      }
    }).catch(function () { doClose(); });
  });

  /* -----------------------------------------------------------
     Public API (expose to pages.js and console)
     ----------------------------------------------------------- */
  window.App = {
    switchView: switchView,
    subscribe: subscribe,
    get currentView() { return currentView; },
    get api() { return window.api; },
  };

  /* -----------------------------------------------------------
      Ripple effect (delegated, respects prefers-reduced-motion)
      ----------------------------------------------------------- */
  var reduceMotion = false;
  try {
    reduceMotion = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  } catch (e) { /* matchMedia unavailable */ }
  document.addEventListener('click', function (e) {
    if (reduceMotion) return;
    var btn = e.target.closest('.btn');
    if (!btn || btn.disabled) return;
    var rect = btn.getBoundingClientRect();
    var size = Math.max(rect.width, rect.height);
    var ripple = document.createElement('span');
    ripple.className = 'ripple';
    ripple.style.width = ripple.style.height = size + 'px';
    ripple.style.left = (e.clientX - rect.left - size / 2) + 'px';
    ripple.style.top = (e.clientY - rect.top - size / 2) + 'px';
    btn.appendChild(ripple);
    setTimeout(function () {
      if (ripple.parentNode) ripple.parentNode.removeChild(ripple);
    }, 600);
  });

  /* -----------------------------------------------------------
      Init
      ----------------------------------------------------------- */
  initSidebar();
  // 启动固定显示首页(welcome);不恢复上次页面

})();
