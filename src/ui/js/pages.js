/**
 * pages.js - 4 pages' rendering + IPC wiring per CONTRACT.md
 * Depends on: window.App (app.js), window.api (preload)
 */
(function () {
  'use strict';

  var api = window.api;
  var subscribe = window.App.subscribe;

  /* ── Theme system (global) ── */
  var THEMES = {
    sky:     { accent:'#0284C7', dark:'#0369A1' },
    emerald: { accent:'#059669', dark:'#047857' },
    violet:  { accent:'#7C3AED', dark:'#6D28D9' },
    orange:  { accent:'#EA580C', dark:'#C2410C' },
    rose:    { accent:'#E11D48', dark:'#BE123C' },
    teal:    { accent:'#0F766E', dark:'#115E59' },
  };
  function applyTheme(id) {
    var t = THEMES[id] || THEMES.sky;
    var s = document.documentElement.style;
    s.setProperty('--color-accent', t.accent);
    s.setProperty('--color-accent-dark', t.dark);
  }
  /* Boot: apply stored theme immediately */
  Promise.resolve(api.storeGet('theme')).then(function (id) { if (id) applyTheme(id); }).catch(function () {});

  var ICON_MAP = {
    win11:'#icon-win11',boot:'#icon-boot',software:'#icon-software',
    system:'#icon-settings',disk:'#icon-disk',network:'#icon-network',
    recycle:'#icon-recycle',temp:'#icon-temp',browser:'#icon-browser',
    download:'#icon-download',registry:'#icon-registry',cookie:'#icon-cookie',
    plugin:'#icon-plugin',trace:'#icon-trace',app:'#icon-app',
    shell:'#icon-registry',service:'#icon-service',task:'#icon-task',folder:'#icon-folder',
    chevron:'#icon-chevron',check:'#icon-check',close:'#icon-close',
    settings:'#icon-settings',info:'#icon-info',text:'#icon-text',
  };

  /* Semantic mapping: groupId keywords → icon names.
     Respects g.icon from main process when present. */
  var GROUP_ICON_MAP = {
    recycle_bin: 'recycle',
    recycle: 'recycle',
    sys_temp: 'temp',
    windows_temp: 'temp',
    thumb_cache: 'browser',
    browser_cache: 'browser',
    prefetch: 'speed',
    crash_dumps: 'info',
    error_reports: 'info',
    soft_dist: 'download',
    downloads_old: 'download',
    font_cache: 'text',
    inet_cache: 'network',
    browser_ext: 'plugin',
    sys_plugin: 'plugin',
    software: 'software',
    temp: 'temp',
    network: 'network',
    browser: 'browser',
    download: 'download',
    registry: 'registry',
    cookie: 'cookie',
    task: 'task',
    service: 'service',
    app: 'app',
  };

  function resolveGroupIcon(g) {
    var id = (g.groupId || '').toLowerCase();
    for (var key in GROUP_ICON_MAP) {
      if (id.indexOf(key) !== -1) return GROUP_ICON_MAP[key];
    }
    if (g.icon && g.icon !== 'folder' && ICON_MAP[g.icon]) return g.icon;
    return g.icon || 'folder';
  }

  function iconSvg(name) {
    var src = ICON_MAP[name] || '#icon-info';
    return '<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><use href="' + src + '"/></svg>';
  }

  /* Local Heroicon SVG paths — inlined to avoid Electron file:// protocol issues */
  var SVG_PATHS = {
    home: '<path stroke-linecap="round" stroke-linejoin="round" d="m2.25 12 8.954-8.955c.44-.439 1.152-.439 1.591 0L21.75 12M4.5 9.75v10.125c0 .621.504 1.125 1.125 1.125H9.75v-4.875c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125V21h4.125c.621 0 1.125-.504 1.125-1.125V9.75M8.25 21h8.25"/>',
    'bolt-lightning': '<path stroke-linecap="round" stroke-linejoin="round" d="m3.75 13.5 10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75Z"/>',
    trash: '<path stroke-linecap="round" stroke-linejoin="round" d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>',
    'list-bullet': '<path stroke-linecap="round" stroke-linejoin="round" d="M8.25 6.75h12M8.25 12h12m-12 5.25h12M3.75 6.75h.007v.008H3.75V6.75Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0ZM3.75 12h.007v.008H3.75V12Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0Zm-.375 5.25h.007v.008H3.75v-.008Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0Z"/>',
    'chart-bar-square': '<path stroke-linecap="round" stroke-linejoin="round" d="M7.5 14.25v2.25m3-4.5v4.5m3-6.75v6.75m3-9v9M6 20.25h12A2.25 2.25 0 0 0 20.25 18V6A2.25 2.25 0 0 0 18 3.75H6A2.25 2.25 0 0 0 3.75 6v12A2.25 2.25 0 0 0 6 20.25Z"/>',
    sparkles: '<path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904 9 18.75l-.813-2.846a4.5 4.5 0 0 0-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 0 0 3.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 0 0 3.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 0 0-3.09 3.09ZM18.259 8.715 18 9.75l-.259-1.035a3.375 3.375 0 0 0-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 0 0 2.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 0 0 2.456 2.456L21.75 6l-1.035.259a3.375 3.375 0 0 0-2.456 2.456ZM16.894 20.567 16.5 21.75l-.394-1.183a2.25 2.25 0 0 0-1.423-1.423L13.5 18.75l1.183-.394a2.25 2.25 0 0 0 1.423-1.423l.394-1.183.394 1.183a2.25 2.25 0 0 0 1.423 1.423l1.183.394-1.183.394a2.25 2.25 0 0 0-1.423 1.423Z"/>',
    clock: '<path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>',
    'shield-check': '<path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75 11.25 15 15 9.75m-3-7.036A11.959 11.959 0 0 1 3.598 6 11.99 11.99 0 0 0 3 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285Z"/>',
    'squares-2x2': '<path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6A2.25 2.25 0 0 1 6 3.75h2.25A2.25 2.25 0 0 1 10.5 6v2.25a2.25 2.25 0 0 1-2.25 2.25H6a2.25 2.25 0 0 1-2.25-2.25V6ZM3.75 15.75A2.25 2.25 0 0 1 6 13.5h2.25a2.25 2.25 0 0 1 2.25 2.25V18a2.25 2.25 0 0 1-2.25 2.25H6A2.25 2.25 0 0 1 3.75 18v-2.25ZM13.5 6a2.25 2.25 0 0 1 2.25-2.25H18A2.25 2.25 0 0 1 20.25 6v2.25A2.25 2.25 0 0 1 18 10.5h-2.25a2.25 2.25 0 0 1-2.25-2.25V6ZM13.5 15.75a2.25 2.25 0 0 1 2.25-2.25H18a2.25 2.25 0 0 1 2.25 2.25V18A2.25 2.25 0 0 1 18 20.25h-2.25A2.25 2.25 0 0 1 13.5 18v-2.25Z"/>',
    power: '<path stroke-linecap="round" stroke-linejoin="round" d="M5.636 5.636a9 9 0 1 0 12.728 0M12 3v9"/>',
    window: '<path stroke-linecap="round" stroke-linejoin="round" d="M9 17.25v1.007a3 3 0 0 1-.879 2.122L7.5 21h9l-.621-.621A3 3 0 0 1 15 18.257V17.25m6-12V15a2.25 2.25 0 0 1-2.25 2.25H5.25A2.25 2.25 0 0 1 3 15V5.25m18 0A2.25 2.25 0 0 0 18.75 3H5.25A2.25 2.25 0 0 0 3 5.25m18 0V12a2.25 2.25 0 0 1-2.25 2.25H5.25A2.25 2.25 0 0 1 3 12V5.25"/>',
    'cog-6-tooth': '<path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 0 1 0 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 0 1 0-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28Z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z"/>',
    'hard-drive': '<path stroke-linecap="round" stroke-linejoin="round" d="M5.25 14.25h13.5m-13.5 0a3 3 0 0 1-3-3m3 3a3 3 0 1 0 0 6h13.5a3 3 0 1 0 0-6m-16.5-3a3 3 0 0 1 3-3h13.5a3 3 0 0 1 3 3m-19.5 0a4.5 4.5 0 0 1 .9-2.7L5.737 5.1a3.375 3.375 0 0 1 2.7-1.35h7.126c1.062 0 2.062.5 2.7 1.35l2.587 3.45a4.5 4.5 0 0 1 .9 2.7m0 0a3 3 0 0 1-3 3m0 3h.008v.008h-.008v-.008Zm0-6h.008v.008h-.008v-.008Zm-3 6h.008v.008h-.008v-.008Zm0-6h.008v.008h-.008v-.008Z"/>',
    wifi: '<path stroke-linecap="round" stroke-linejoin="round" d="M8.288 15.038a5.25 5.25 0 0 1 7.424 0M5.106 11.856c3.807-3.808 9.98-3.808 13.788 0M1.924 8.674c5.565-5.565 14.587-5.565 20.152 0M12.53 18.22l-.53.53-.53-.53a.75.75 0 0 1 1.06 0Z"/>',
    folder: '<path stroke-linecap="round" stroke-linejoin="round" d="M2.25 12.75V12A2.25 2.25 0 0 1 4.5 9.75h15A2.25 2.25 0 0 1 21.75 12v.75m-8.69-6.44-2.12-2.12a1.5 1.5 0 0 0-1.061-.44H4.5A2.25 2.25 0 0 0 2.25 6v12a2.25 2.25 0 0 0 2.25 2.25h15A2.25 2.25 0 0 0 21.75 18V9a2.25 2.25 0 0 0-2.25-2.25h-5.379a1.5 1.5 0 0 1-1.06-.44Z"/>',
    recycle: '<path stroke-linecap="round" stroke-linejoin="round" d="M9 15 3 9m0 0 6-6M3 9h12a6 6 0 0 1 0 12h-3"/>',
    temp: '<path stroke-linecap="round" stroke-linejoin="round" d="M15.362 5.214A8.252 8.252 0 0 1 12 21 8.25 8.25 0 0 1 6.038 7.047 8.287 8.287 0 0 0 9 9.601a8.983 8.983 0 0 1 3.361-6.867 8.21 8.21 0 0 0 3 2.48Z"/><path stroke-linecap="round" stroke-linejoin="round" d="M12 18a3.75 3.75 0 0 0 .495-7.468 5.99 5.99 0 0 0-1.925 3.547 5.975 5.975 0 0 1-2.133-1.001A3.75 3.75 0 0 0 12 18Z"/>',
    browser: '<path stroke-linecap="round" stroke-linejoin="round" d="M12 21a9.004 9.004 0 0 0 8.716-6.747M12 21a9.004 9.004 0 0 1-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 0 1 7.843 4.582M12 3a8.997 8.997 0 0 0-7.843 4.582m15.686 0A11.953 11.953 0 0 1 12 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0 1 21 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0 1 12 16.5c-3.162 0-6.133-.815-8.716-2.247m0 0A9.015 9.015 0 0 1 3 12c0-1.605.42-3.113 1.157-4.418"/>',
    download: '<path stroke-linecap="round" stroke-linejoin="round" d="M3 16.5v2.25A2.25 2.25 0 0 0 5.25 21h13.5A2.25 2.25 0 0 0 21 18.75V16.5M16.5 12 12 16.5m0 0L7.5 12m4.5 4.5V3"/>',
    registry: '<path stroke-linecap="round" stroke-linejoin="round" d="M15.75 5.25a3 3 0 0 1 3 3m3 0a6 6 0 0 1-7.029 5.912c-.563-.097-1.159.026-1.563.43L10.5 17.25H8.25v2.25H6v2.25H2.25v-2.818c0-.597.237-1.17.659-1.591l6.499-6.499c.404-.404.527-1 .43-1.563A6 6 0 1 1 21.75 8.25Z"/>',
    cookie: '<path stroke-linecap="round" stroke-linejoin="round" d="M2.036 12.322a1.012 1.012 0 0 1 0-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178Z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z"/>',
    task: '<path stroke-linecap="round" stroke-linejoin="round" d="M11.35 3.836c-.065.21-.1.433-.1.664 0 .414.336.75.75.75h4.5a.75.75 0 0 0 .75-.75 2.25 2.25 0 0 0-.1-.664m-5.8 0A2.251 2.251 0 0 1 13.5 2.25H15c1.012 0 1.867.668 2.15 1.586m-5.8 0c-.376.023-.75.05-1.124.08C9.095 4.01 8.25 4.973 8.25 6.108V8.25m8.9-4.414c.376.023.75.05 1.124.08 1.131.094 1.976 1.057 1.976 2.192V16.5A2.25 2.25 0 0 1 18 18.75h-2.25m-7.5-10.5H4.875c-.621 0-1.125.504-1.125 1.125v11.25c0 .621.504 1.125 1.125 1.125h9.75c.621 0 1.125-.504 1.125-1.125V18.75m-7.5-10.5h6.375c.621 0 1.125.504 1.125 1.125v9.375m-8.25-3 1.5 1.5 3-3.75"/>',
    service: '<path stroke-linecap="round" stroke-linejoin="round" d="M11.42 15.17 17.25 21A2.652 2.652 0 0 0 21 17.25l-5.877-5.877M11.42 15.17l2.496-3.03c.317-.384.74-.626 1.208-.766M11.42 15.17l-4.655 5.653a2.548 2.548 0 1 1-3.586-3.586l6.837-5.63m5.108-.233c.55-.164 1.163-.188 1.743-.14a4.5 4.5 0 0 0 4.486-6.336l-3.276 3.277a3.004 3.004 0 0 1-2.25-2.25l3.276-3.276a4.5 4.5 0 0 0-6.336 4.486c.091 1.076-.071 2.264-.904 2.95l-.102.085m-1.745 1.437L5.909 7.5H4.5L2.25 3.75l1.5-1.5L7.5 4.5v1.409l4.26 4.26m-1.745 1.437 1.745-1.437m6.615 8.206L15.75 15.75M4.867 19.125h.008v.008h-.008v-.008Z"/>',
    app: '<path stroke-linecap="round" stroke-linejoin="round" d="M3.75 6A2.25 2.25 0 0 1 6 3.75h2.25A2.25 2.25 0 0 1 10.5 6v2.25a2.25 2.25 0 0 1-2.25 2.25H6a2.25 2.25 0 0 1-2.25-2.25V6ZM3.75 15.75A2.25 2.25 0 0 1 6 13.5h2.25a2.25 2.25 0 0 1 2.25 2.25V18a2.25 2.25 0 0 1-2.25 2.25H6A2.25 2.25 0 0 1 3.75 18v-2.25ZM13.5 6a2.25 2.25 0 0 1 2.25-2.25H18A2.25 2.25 0 0 1 20.25 6v2.25A2.25 2.25 0 0 1 18 10.5h-2.25a2.25 2.25 0 0 1-2.25-2.25V6ZM13.5 15.75a2.25 2.25 0 0 1 2.25-2.25H18a2.25 2.25 0 0 1 2.25 2.25V18A2.25 2.25 0 0 1 18 20.25h-2.25A2.25 2.25 0 0 1 13.5 18v-2.25Z"/>',
    plugin: '<path stroke-linecap="round" stroke-linejoin="round" d="M14.25 6.087c0-.355.186-.676.401-.959.221-.29.349-.634.349-1.003 0-1.036-1.007-1.875-2.25-1.875s-2.25.84-2.25 1.875c0 .369.128.713.349 1.003.215.283.401.604.401.959v0a.64.64 0 0 1-.657.643 48.39 48.39 0 0 1-4.163-.3c.186 1.613.293 3.25.315 4.907a.656.656 0 0 1-.658.663v0c-.355 0-.676-.186-.959-.401a1.647 1.647 0 0 0-1.003-.349c-1.036 0-1.875 1.007-1.875 2.25s.84 2.25 1.875 2.25c.369 0 .713-.128 1.003-.349.283-.215.604-.401.959-.401v0c.31 0 .555.26.532.57a48.039 48.039 0 0 1-.642 5.056c1.518.19 3.058.309 4.616.354a.64.64 0 0 0 .657-.643v0c0-.355-.186-.676-.401-.959a1.647 1.647 0 0 1-.349-1.003c0-1.035 1.008-1.875 2.25-1.875 1.243 0 2.25.84 2.25 1.875 0 .369-.128.713-.349 1.003-.215.283-.4.604-.4.959v0c0 .333.277.599.61.58a48.1 48.1 0 0 0 5.427-.63 48.05 48.05 0 0 0 .582-4.717.532.532 0 0 0-.533-.57v0c-.355 0-.676.186-.959.401-.29.221-.634.349-1.003.349-1.035 0-1.875-1.007-1.875-2.25s.84-2.25 1.875-2.25c.37 0 .713.128 1.003.349.283.215.604.401.96.401v0a.656.656 0 0 0 .658-.663 48.422 48.422 0 0 0-.37-5.36c-1.886.342-3.81.574-5.766.689a.578.578 0 0 1-.61-.58v0Z"/>',
    info: '<path stroke-linecap="round" stroke-linejoin="round" d="m11.25 11.25.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>',
    text: '<path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z"/>',
    network: '<path stroke-linecap="round" stroke-linejoin="round" d="M9.348 14.652a3.75 3.75 0 0 1 0-5.304m5.304 0a3.75 3.75 0 0 1 0 5.304m-7.425 2.121a6.75 6.75 0 0 1 0-9.546m9.546 0a6.75 6.75 0 0 1 0 9.546M5.106 18.894c-3.808-3.807-3.808-9.98 0-13.788m13.788 0c3.808 3.807 3.808 9.98 0 13.788M12 12h.008v.008H12V12Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0Z"/>',
    software: '<path stroke-linecap="round" stroke-linejoin="round" d="M8.25 3v1.5M4.5 8.25H3m18 0h-1.5M4.5 12H3m18 0h-1.5m-15 3.75H3m18 0h-1.5M8.25 19.5V21M12 3v1.5m0 15V21m3.75-18v1.5m0 15V21m-9-1.5h10.5a2.25 2.25 0 0 0 2.25-2.25V6.75a2.25 2.25 0 0 0-2.25-2.25H6.75A2.25 2.25 0 0 0 4.5 6.75v10.5a2.25 2.25 0 0 0 2.25 2.25Zm.75-12h9v9h-9v-9Z"/>',
    file: '<path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m2.25 0H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z"/>',
    image: '<path stroke-linecap="round" stroke-linejoin="round" d="m2.25 15.75 5.159-5.159a2.25 2.25 0 0 1 3.182 0l5.159 5.159m-1.5-1.5 1.409-1.409a2.25 2.25 0 0 1 3.182 0l2.909 2.909m-18 3.75h16.5a1.5 1.5 0 0 0 1.5-1.5V6a1.5 1.5 0 0 0-1.5-1.5H3.75A1.5 1.5 0 0 0 2.25 6v12a1.5 1.5 0 0 0 1.5 1.5Zm10.5-11.25h.008v.008h-.008V8.25Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0Z"/>',
    system: '<path stroke-linecap="round" stroke-linejoin="round" d="M9.594 3.94c.09-.542.56-.94 1.11-.94h2.593c.55 0 1.02.398 1.11.94l.213 1.281c.063.374.313.686.645.87.074.04.147.083.22.127.325.196.72.257 1.075.124l1.217-.456a1.125 1.125 0 0 1 1.37.49l1.296 2.247a1.125 1.125 0 0 1-.26 1.431l-1.003.827c-.293.241-.438.613-.43.992a7.723 7.723 0 0 1 0 .255c-.008.378.137.75.43.991l1.004.827c.424.35.534.955.26 1.43l-1.298 2.247a1.125 1.125 0 0 1-1.369.491l-1.217-.456c-.355-.133-.75-.072-1.076.124a6.47 6.47 0 0 1-.22.128c-.331.183-.581.495-.644.869l-.213 1.281c-.09.543-.56.94-1.11.94h-2.594c-.55 0-1.019-.398-1.11-.94l-.213-1.281c-.062-.374-.312-.686-.644-.87a6.52 6.52 0 0 1-.22-.127c-.325-.196-.72-.257-1.076-.124l-1.217.456a1.125 1.125 0 0 1-1.369-.49l-1.297-2.247a1.125 1.125 0 0 1 .26-1.431l1.004-.827c.292-.24.437-.613.43-.991a6.932 6.932 0 0 1 0-.255c.007-.38-.138-.751-.43-.992l-1.004-.827a1.125 1.125 0 0 1-.26-1.43l1.297-2.247a1.125 1.125 0 0 1 1.37-.491l1.216.456c.356.133.751.072 1.076-.124.072-.044.146-.086.22-.128.332-.183.582-.495.644-.869l.214-1.28Z"/><path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z"/>',
    speed: '<path stroke-linecap="round" stroke-linejoin="round" d="m3.75 13.5 10.5-11.25L12 10.5h8.25L9.75 21.75 12 13.5H3.75Z"/>',
    chevron: '<path stroke-linecap="round" stroke-linejoin="round" d="m8.25 4.5 7.5 7.5-7.5 7.5"/>',
    check: '<path stroke-linecap="round" stroke-linejoin="round" d="m4.5 12.75 6 6 9-13.5"/>',
    'plus-circle': '<path stroke-linecap="round" stroke-linejoin="round" d="M12 9v6m3-3H9m12 0a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>',
    'arrow-down-circle': '<path stroke-linecap="round" stroke-linejoin="round" d="m9 12.75 3 3m0 0 3-3m-3 3v-7.5M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>',
    'bug-ant': '<path stroke-linecap="round" stroke-linejoin="round" d="M12 12.75c1.148 0 2.278.08 3.383.237 1.037.146 1.866.966 1.866 2.013 0 3.728-2.35 6.75-5.25 6.75S6.75 18.728 6.75 15c0-1.046.83-1.867 1.866-2.013A24.204 24.204 0 0 1 12 12.75Zm0 0c2.883 0 5.647.508 8.207 1.44a23.91 23.91 0 0 1-1.152 6.06M12 12.75c-2.883 0-5.647.508-8.208 1.44.125 2.104.52 4.136 1.153 6.06M12 12.75a2.25 2.25 0 0 0 2.248-2.354M12 12.75a2.25 2.25 0 0 1-2.248-2.354M12 8.25c.995 0 1.971-.08 2.922-.236.403-.066.74-.358.795-.762a3.778 3.778 0 0 0-.399-2.25M12 8.25c-.995 0-1.97-.08-2.922-.236-.402-.066-.74-.358-.795-.762a3.734 3.734 0 0 1 .4-2.253M12 8.25a2.25 2.25 0 0 0-2.248 2.146M12 8.25a2.25 2.25 0 0 1 2.248 2.146M8.683 5a6.032 6.032 0 0 1-1.155-1.002c.07-.63.27-1.222.574-1.747m.581 2.749A3.75 3.75 0 0 1 15.318 5m0 0c.427-.283.815-.62 1.155-.999a4.471 4.471 0 0 0-.575-1.752M4.921 6a24.048 24.048 0 0 0-.392 3.314c1.668.546 3.416.914 5.223 1.082M19.08 6c.205 1.08.337 2.187.392 3.314a23.882 23.882 0 0 1-5.223 1.082"/>',
    'exclamation-triangle': '<path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>',
    'document-text': '<path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 0 0-3.375-3.375h-1.5A1.125 1.125 0 0 1 13.5 7.125v-1.5a3.375 3.375 0 0 0-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 0 0-9-9Z"/>',
    globe: '<path stroke-linecap="round" stroke-linejoin="round" d="M12 21a9.004 9.004 0 0 0 8.716-6.747M12 21a9.004 9.004 0 0 1-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 0 1 7.843 4.582M12 3a8.997 8.997 0 0 0-7.843 4.582m15.686 0A11.953 11.953 0 0 1 12 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0 1 21 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0 1 12 16.5c-3.162 0-6.133-.815-8.716-2.247m0 0A9.015 9.015 0 0 1 3 12c0-1.605.42-3.113 1.157-4.418"/>',
    'image-square': '<path stroke-linecap="round" stroke-linejoin="round" d="m2.25 15.75 5.159-5.159a2.25 2.25 0 0 1 3.182 0l5.159 5.159m-1.5-1.5 1.409-1.409a2.25 2.25 0 0 1 3.182 0l2.909 2.909m-18 3.75h16.5a1.5 1.5 0 0 0 1.5-1.5V6a1.5 1.5 0 0 0-1.5-1.5H3.75A1.5 1.5 0 0 0 2.25 6v12a1.5 1.5 0 0 0 1.5 1.5Zm10.5-11.25h.008v.008h-.008V8.25Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0Z"/>',
    'empty-state': '<path stroke-linecap="round" stroke-linejoin="round" d="M17.982 18.725A7.488 7.488 0 0 0 12 15.75a7.488 7.488 0 0 0-5.982 2.975m11.963 0a9 9 0 1 0-11.963 0m11.963 0A8.966 8.966 0 0 1 12 21a8.966 8.966 0 0 1-5.982-2.275M15 9.75a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z"/>',
    close: '<path stroke-linecap="round" stroke-linejoin="round" d="M6 18 18 6M6 6l12 12"/>',
    trace: '<path stroke-linecap="round" stroke-linejoin="round" d="M9.348 14.652a3.75 3.75 0 0 1 0-5.304m5.304 0a3.75 3.75 0 0 1 0 5.304m-7.425 2.121a6.75 6.75 0 0 1 0-9.546m9.546 0a6.75 6.75 0 0 1 0 9.546M5.106 18.894c-3.808-3.807-3.808-9.98 0-13.788m13.788 0c3.808 3.807 3.808 9.98 0 13.788M12 12h.008v.008H12V12Zm.375 0a.375.375 0 1 1-.75 0 .375.375 0 0 1 .75 0Z"/>',
  };

  function localSvg(name, size) {
    var s = size || 16;
    var path = SVG_PATHS[name] || SVG_PATHS.info;
    return '<svg width="' + s + '" height="' + s + '" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" class="local-svg-icon">' + path + '</svg>';
  }

  function formatSize(bytes) {
    if (bytes == null || isNaN(bytes)) return '--';
    if (bytes >= 1073741824) return (bytes / 1073741824).toFixed(1) + ' GB';
    if (bytes >= 1048576) return (bytes / 1048576).toFixed(1) + ' MB';
    if (bytes >= 1024) return (bytes / 1024).toFixed(1) + ' KB';
    return bytes + ' B';
  }

  function formatCleaned(mb) {
    if (mb >= 1024) return (mb / 1024).toFixed(1) + 'GB';
    return mb.toFixed(1) + 'MB';
  }

  // 截断文本，最多保留 maxLen 个字符，超出显示...
  function truncate(text, maxLen) {
    if (!text) return '';
    var t = String(text);
    if (t.length <= maxLen) return t;
    return t.substring(0, maxLen) + '...';
  }

  var _toastTimer = null;
  function showToast(msg) {
    var t = document.getElementById('globalToast');
    if (!t) {
      t = document.createElement('div');
      t.id = 'globalToast';
      t.className = 'global-toast';
      document.body.appendChild(t);
    }
    t.textContent = msg;
    t.classList.add('show');
    if (_toastTimer) clearTimeout(_toastTimer);
    _toastTimer = setTimeout(function () {
      t.classList.remove('show');
    }, 2400);
  }

  function formatStartupTime(ms) {
    if (ms == null || isNaN(ms)) return '--';
    if (ms >= 1000) return (ms / 1000).toFixed(1) + 's';
    return Math.round(ms) + 'ms';
  }

  /* CountUp animation — smooth number counter with easeOutQuad using requestAnimationFrame */
  function countUp(el, target, opts) {
    if (!el || !target) return;
    opts = opts || {};
    var duration = opts.duration || 1600;
    var decimals = opts.decimals || 0;
    var suffix = opts.suffix || '';
    var prefix = opts.prefix || '';
    var startTime = null;
    var easeOutQuad = function (t) { return t * (2 - t); };
    var animate = function (timestamp) {
      if (!startTime) startTime = timestamp;
      var elapsed = timestamp - startTime;
      var progress = Math.min(elapsed / duration, 1);
      var current = target * easeOutQuad(progress);
      el.textContent = prefix + (decimals > 0 ? current.toFixed(decimals) : Math.round(current)) + suffix;
      if (progress < 1) {
        requestAnimationFrame(animate);
      } else {
        el.textContent = prefix + (decimals > 0 ? target.toFixed(decimals) : target) + suffix;
      }
    };
    requestAnimationFrame(animate);
  }

  function esc(s) {
    if (!s) return '';
    var d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
  }

  /* ===========================================================
     WELCOME PAGE
     =========================================================== */
  function getGreeting() {
    var h = new Date().getHours();
    if (h >= 5 && h < 11) return '早上好，新的一天从清理开始';
    if (h >= 11 && h < 13) return '中午好，顺手清一清电脑吧';
    if (h >= 13 && h < 18) return '下午好，来给系统减减负';
    if (h >= 18 && h < 23) return '晚上好，睡前清掉垃圾更清爽';
    return '夜深了，给电脑也放个假吧';
  }

  function renderWelcomePage(stats) {
    var totalMB = stats && stats.totalCleanedMB ? stats.totalCleanedMB : 0;

    var elTotal = document.getElementById('statTotalCleaned');

    // CountUp animation for the cumulative stat
    if (elTotal) {
      var totalVal = totalMB >= 1024 ? totalMB / 1024 : totalMB;
      var totalUnit = totalMB >= 1024 ? 'GB' : 'MB';
      countUp(elTotal, totalVal, { duration: 1600, decimals: 1, suffix: totalUnit });
    }

    var welcomeEl = document.getElementById('welcomeStats');
    if (welcomeEl) {
      welcomeEl.textContent = getGreeting();
    }

    // Recent activity
    var history = stats && stats.history ? stats.history : [];
    var recentActivity = document.getElementById('recentActivity');
    var recentList = document.getElementById('recentList');
    if (history.length > 0 && recentList) {
      var icons = {
        clean: localSvg('trash', 16),
        optimize: localSvg('sparkles', 16),
        startup: localSvg('list-bullet', 16),
      };
      var types = ['clean', 'optimize', 'startup'];
      var labels = { clean: '系统清理', optimize: '一键优化', startup: '启动项优化' };
      var items = history.slice(-3).reverse();
      var h = '';
      items.forEach(function (item, idx) {
        var typeKey = types[idx % types.length];
        var date = item.date || '';
        var sizeMB = item.sizeMB || 0;
        h += '<div class="recent-item">'
          + '<span class="recent-item-icon">' + (icons[typeKey] || icons.clean) + '</span>'
          + '<span class="recent-item-date">' + date + '</span>'
          + '<span class="recent-item-type">' + (labels[typeKey] || '系统清理') + '</span>'
          + '<span class="recent-item-result">' + sizeMB.toFixed(1) + ' MB 已释放</span>'
          + '</div>';
      });
      recentList.innerHTML = h;
      if (recentActivity) recentActivity.style.display = '';
    }
  }

  api.getAppStats().then(renderWelcomePage).catch(function () {
    var welcomeEl = document.getElementById('welcomeStats');
    if (welcomeEl) welcomeEl.textContent = getGreeting();
  });

  // 实时刷新首页累计数据：切回首页时、以及清理完成后各拉取一次
  function refreshWelcomeStats() {
    api.getAppStats().then(renderWelcomePage).catch(function () {});
  }
  document.addEventListener('viewchange', function (e) {
    if (e && e.detail && e.detail.view === 'welcome') refreshWelcomeStats();
  });

  /* ===========================================================
     SPEEDUP PAGE — Card Layout
     =========================================================== */
  var sp = { scanning: false, done: false, items: [], total: 0, expanded: null };
  var spTitle = document.getElementById('speedupTitle');
  var spAction = document.getElementById('speedupAction');
  var spProgWrap = document.getElementById('speedupProgressWrap');
  var spProgFill = document.getElementById('speedupProgressFill');
  var spItems = document.getElementById('speedupItems');

  var SP_DEFS = [
    { id:'win11',name:'Win11\u52A0\u901F\u9879',desc:'\u4F18\u5316Win11\u7CFB\u7EDF\u548C\u5185\u5B58\u8BBE\u7F6E',icon:'squares-2x2' },
    { id:'boot',name:'\u5F00\u673A\u52A0\u901F',desc:'\u4F18\u5316\u8F6F\u4EF6\u81EA\u542F\u72B6\u6001',icon:'power' },
    { id:'software',name:'\u8F6F\u4EF6\u8FD0\u884C\u52A0\u901F',desc:'\u9000\u51FA\u6682\u4E0D\u4F7F\u7528\u7684\u8F6F\u4EF6',icon:'window' },
    { id:'system',name:'\u7CFB\u7EDF\u52A0\u901F',desc:'\u4F18\u5316\u7CFB\u7EDF\u548C\u5185\u5B58\u8BBE\u7F6E',icon:'cog-6-tooth' },
    { id:'disk',name:'\u786C\u76D8\u52A0\u901F',desc:'\u4F18\u5316\u786C\u76D8\u4F20\u8F93\u6548\u7387',icon:'hard-drive' },
    { id:'network',name:'\u7F51\u7EDC\u52A0\u901F',desc:'\u4F18\u5316\u7F51\u7EDC\u914D\u7F6E\u548C\u6027\u80FD',icon:'wifi' },
  ];

  function renderSP() {
    var h = '';
    SP_DEFS.forEach(function (d) {
      var data = sp.items.filter(function(x){return x.id===d.id;})[0] || null;
      var status = data ? data.status : (sp.scanning ? 'scanning' : '');
      var found = data ? data.found : 0;
      var exp = sp.expanded === d.id;
      var details = data && data.items ? data.items : [];
      var st = '', sc = '';
      if (status === 'scanning') { st = '\u6B63\u5728\u626B\u63CF\u2026'; sc = 'scanning'; }
      else if (status === 'found') { st = '\u53D1\u73B0 ' + found + ' \u9879'; sc = 'found'; }
      else if (status === 'optimized') { st = '\u5DF2\u4F18\u5316'; sc = 'optimized'; }
      else if (status === 'clean') { st = '\u65E0\u9700\u4F18\u5316'; sc = 'clean'; }

      h += '<div class="speedup-card' + (exp ? ' expanded' : '') + (status === 'found' ? ' found' : '') + (status === 'optimized' ? ' optimized' : '') + '" data-id="' + d.id + '">';
      h += '<div class="speedup-card-icon">' + localSvg(d.icon, 24) + '</div>';
      h += '<div class="speedup-card-title">' + esc(d.name) + '</div>';
      h += '<div class="speedup-card-desc">' + esc(d.desc) + '</div>';
      h += '<div class="speedup-card-status ' + sc + '"><span class="speedup-card-status-dot"></span>' + st + '</div>';

      h += '<div class="speedup-card-detail">';
      if (details.length === 0 && status === 'scanning') {
        h += '<div class="placeholder-row"><div class="placeholder-bar" style="width:60%;height:10px"></div></div>';
        h += '<div class="placeholder-row"><div class="placeholder-bar" style="width:40%;height:10px"></div></div>';
      } else if (details.length === 0) {
        h += '<p class="text-tertiary" style="font-size:12px;padding:4px 0">\u672A\u53D1\u73B0\u9700\u8981\u5904\u7406\u7684\u9879</p>';
      } else {
        details.forEach(function (it) {
          h += '<div class="speedup-card-detail-item">';
          h += '<div><div class="speedup-card-detail-name">' + esc(it.name) + '</div>';
          h += '<div class="speedup-card-detail-desc">' + esc(it.detail || it.desc || '') + '</div></div>';
          h += '<span class="speedup-card-detail-suggestion">' + esc(it.suggestion || '') + '</span>';
          h += '</div>';
        });
      }
      h += '</div></div>';
    });
    spItems.innerHTML = h;
    spItems.querySelectorAll('.speedup-card').forEach(function (el) {
      el.addEventListener('click', function () {
        var id = this.getAttribute('data-id');
        sp.expanded = sp.expanded === id ? null : id;
        renderSP();
      });
    });
  }

  function updateSPHeader() {
    if (sp.done) {
      spTitle.innerHTML = '扫描完成，发现 <span class="countup-inline">0</span> 个优化项';
      var countEl = spTitle.querySelector('.countup-inline');
      if (countEl) countUp(countEl, sp.total, { duration: 800 });
      spAction.textContent = '一键优化';
      spProgWrap.style.display = 'none';
    } else if (sp.scanning) {
      spTitle.innerHTML = '扫描中，发现 <span class="countup-inline">0</span> 个优化项...';
      var countEl2 = spTitle.querySelector('.countup-inline');
      if (countEl2 && sp.total > 0) countUp(countEl2, sp.total, { duration: 400 });
      spAction.textContent = '取消扫描';
      spProgWrap.style.display = '';
    } else {
      spTitle.textContent = '扫描中，发现 0 个优化项...';
      spAction.textContent = '取消扫描';
      spProgWrap.style.display = '';
    }
  }

  function startSPScan() {
    sp.scanning = true; sp.done = false; sp.items = []; sp.total = 0; sp.expanded = null;
    updateSPHeader(); renderSP();
    spProgWrap.style.display = '';
    spProgFill.className = 'progress-fill indeterminate';
    api.speedupScan();
  }

  spAction.addEventListener('click', function () {
    if (sp.done) {
      var ids = [];
      sp.items.forEach(function (it) {
        (it.items || []).forEach(function (f) { ids.push(f.id); });
      });
      api.speedupOptimize(ids).then(function () {
        sp.items.forEach(function (it) { it.status = 'optimized'; it.items = []; });
        sp.total = 0;
        updateSPHeader(); renderSP();
      });
    } else {
      api.speedupCancel();
      sp.scanning = false;
      updateSPHeader(); renderSP();
    }
  });

  subscribe('speedup:progress', function (p) {
    if (!sp.scanning) return;
    if (p.itemId) {
      var ex = sp.items.filter(function(x){return x.id===p.itemId;})[0];
      if (ex) { ex.status = 'scanning'; }
      else { sp.items.push({id:p.itemId,name:p.itemId,desc:p.message||'',icon:p.itemId,status:'scanning',found:0,items:[]}); }
      renderSP();
    }
  });

  subscribe('speedup:item-status', function (p) {
    var ex = sp.items.filter(function(x){return x.id===p.itemId;})[0];
    if (ex) {
      ex.status = p.status;
      ex.found = p.found || 0;
      if (p.desc) ex.desc = p.desc;
    }
    sp.total = 0;
    sp.items.forEach(function (it) { sp.total += (it.found || 0); });
    updateSPHeader(); renderSP();
  });

  subscribe('speedup:done', function (p) {
    sp.scanning = false; sp.done = true;
    sp.items = p.items || [];
    sp.total = p.total || 0;
    spProgFill.className = 'progress-fill';
    spProgFill.style.width = '100%';
    updateSPHeader(); renderSP();
  });

  /* Auto-start scan when navigating to speedup */
  document.addEventListener('viewchange', function (e) {
    if (e.detail.view === 'speedup' && !sp.scanning && !sp.done) {
      startSPScan();
    }
    if (e.detail.view === 'clean') { loadCleanTab(); positionTabIndicator(clTabs); }
    if (e.detail.view === 'startup') { loadStartupTab(); positionTabIndicator(stTabs); }
  });

  // 窗口尺寸变化时重新定位指示条（标签宽度/位置可能改变）
  window.addEventListener('resize', function () {
    positionTabIndicator(clTabs);
    positionTabIndicator(stTabs);
  });

  /* ===========================================================
     CLEAN PAGE
     =========================================================== */
  var cl = { tab: 'trash', scanning: false, done: false, groups: [], path: '', cache: {}, scanKey: 0, mode: 'browse', phase: 0 };
  var clTabs = document.getElementById('cleanTabs');
  var clPath = document.getElementById('cleanScanPath');
  var clProgWrap = document.getElementById('cleanProgressWrap');
  var clProgFill = document.getElementById('cleanProgressFill');
  var clGroups = document.getElementById('cleanGroups');
  var clCancelBtn = document.getElementById('cleanCancelBtn');
  var clExecBtn = document.getElementById('cleanExecuteBtn');
  var clFloatingBar = document.getElementById('cleanFloatingBar');
  var clRescanBtn = document.getElementById('cleanRescanBtn');
  var clHeaderInfo = document.getElementById('cleanHeaderInfo');
  var clHeaderTitle = document.getElementById('cleanHeaderTitle');
  var clHeaderSize = document.getElementById('cleanHeaderSize');
  var clHeaderStatus = document.getElementById('cleanHeaderStatus');
  var clFloatingText = document.getElementById('cleanFloatingText');
  var clFloatingSubText = document.getElementById('cleanFloatingSubText');
  var cleanFangxinBtn = document.getElementById('cleanFangxinBtn');
  var clItemFilesCache = {}; // groupId:itemId -> string[] 详情文件列表缓存

  function calcSelectedSize(items) {
    return (items || []).filter(function(x){return x.checked;}).reduce(function(s, it){return s + (it.size || 0);}, 0);
  }

  // 滑动指示条：根据当前选中 tab 的位置/宽度移动到底部指示条
  function positionTabIndicator(tabsEl) {
    if (!tabsEl) return;
    // 用 rAF 确保目标视图已布局（视图切换/动画期间 offset 才准确）
    requestAnimationFrame(function () {
      var indicator = tabsEl.querySelector('.tab-indicator');
      var active = tabsEl.querySelector('.tab.active');
      if (!indicator || !active) return;
      indicator.style.width = active.offsetWidth + 'px';
      indicator.style.transform = 'translateX(' + active.offsetLeft + 'px)';
    });
  }

  function loadCleanTab(force) {
    cl.mode = 'browse'; cl.phase = 0;
    if (cleanFangxinBtn) { cleanFangxinBtn.disabled = false; cleanFangxinBtn.style.display = ''; }
    // 浏览模式：隐藏“立即清理”，仅保留“放心清理”作为主操作
    if (clExecBtn) { clExecBtn.style.display = 'none'; clExecBtn.disabled = false; clExecBtn.textContent = '立即清理'; }
    cl.tab = (clTabs.querySelector('.tab.active') || {}).getAttribute('data-tab') || 'trash';
    // 独立缓存：切换 tab 时若该 tab 已扫描过，直接复用结果，不重复扫描
    var cached = cl.cache[cl.tab];
    if (cached && cached.groups && !force) {
      cl.scanning = false; cl.done = true;
      cl.groups = cached.groups;
      cl.path = '';
      clProgWrap.style.display = 'none';
      if (clFloatingBar) clFloatingBar.style.display = '';
      renderCleanGroups();
      return;
    }
    // 快速切换时直接发起新扫描：主进程 supersede 旧扫描（按 scanKey 失效），
    // 无需取消-重试补丁
    cl.scanning = true; cl.done = false; cl.groups = []; cl.path = '';
    clItemFilesCache = {};
    // 立即清空 DOM，防止上一个 tab 的结果残留显示
    clGroups.innerHTML = '';
    clProgWrap.style.display = '';
    clProgFill.className = 'progress-fill indeterminate';
    clProgFill.style.width = '';
    if (clFloatingBar) clFloatingBar.style.display = 'none';
    if (clRescanBtn) clRescanBtn.style.display = 'none';
    if (clHeaderInfo) clHeaderInfo.style.display = 'none';
    var scanStatus = document.getElementById('cleanScanStatus');
    if (scanStatus) scanStatus.style.display = '';
    renderCleanGroups();
    beginCleanScan();
  }

  // 原生扫描入口：记录主进程返回的 scanKey（唯一事实源），旧回调自动失效
  function beginCleanScan() {
    api.cleanScan(cl.tab).then(function (r) {
      if (r && typeof r.scanKey === 'number') {
        cl.scanKey = r.scanKey;
      }
    }).catch(function () { /* ignore */ });
  }
  // 调试/自动化测试探针：读取当前会话 scanKey
  window.__clGetScanKey = function () { return cl.scanKey; };

  clTabs.querySelectorAll('.tab').forEach(function (t) {
    t.addEventListener('click', function () {
      clTabs.querySelector('.tab.active').classList.remove('active');
      this.classList.add('active');
      cl.tab = this.getAttribute('data-tab');
      positionTabIndicator(clTabs);
      loadCleanTab();
    });
  });

  clCancelBtn.addEventListener('click', function () {
    // 取消：主进程停止 walk（组内探针即时生效）；scanKey 归零使所有在途回调失效
    api.cleanCancel();
    cl.scanKey = 0;
    cl.scanning = false;
    clProgWrap.style.display = 'none';
  });

  if (clRescanBtn) {
    clRescanBtn.addEventListener('click', function () {
      loadCleanTab(true);
    });
  }

  function runClean() {
    var refs = [];
    cl.groups.forEach(function (g) {
      (g.items || []).forEach(function (it) {
        if (it.checked) refs.push({ groupId: g.groupId, itemId: it.id, path: it.path, size: it.size });
      });
    });
    if (refs.length === 0) {
      if (cl.mode === 'fangxin' && cl.phase === 1) { finishPhase1(); }
      return;
    }
    clExecBtn.disabled = true;
    clExecBtn.textContent = '清理中...';
    api.cleanExecute(refs).then(function (r) {
      clExecBtn.disabled = false;
      if (cl.mode === 'fangxin' && cl.phase === 1) {
        finishPhase1();
      } else {
        clExecBtn.textContent = '一键清理';
        if (cl.mode === 'fangxin') { cl.mode = 'browse'; cl.phase = 0; if (cleanFangxinBtn) { cleanFangxinBtn.disabled = false; cleanFangxinBtn.style.display = ''; } }
        if (r && r.ok) {
          if (clFloatingBar) clFloatingBar.style.display = 'none';
          clGroups.innerHTML = '<p class="text-tertiary" style="text-align:center;padding:40px 0">清理完成</p>';
          refreshWelcomeStats();
        }
      }
    }).catch(function () {
      clExecBtn.disabled = false;
      clExecBtn.style.display = (cl.mode === 'fangxin' && cl.phase === 2) ? '' : 'none';
      clExecBtn.textContent = (cl.mode === 'fangxin' && cl.phase === 2) ? '清理所选' : '一键清理';
    });
  }

  function finishPhase1() {
    cl.phase = 2;
    cl.groups = cl.groups.filter(function (g) { return !!g.risky; });
    cl.groups.forEach(function (g) {
      g.checked = false;
      g._expanded = true;
      (g.items || []).forEach(function (it) { it.checked = false; });
    });
    if (cl.groups.length === 0) {
      if (clFloatingBar) clFloatingBar.style.display = 'none';
      clGroups.innerHTML = '<p class="text-tertiary" style="text-align:center;padding:40px 0">放心清理完成，没有需要您确认的项目</p>';
      cl.mode = 'browse'; cl.phase = 0;
      if (cleanFangxinBtn) { cleanFangxinBtn.disabled = false; cleanFangxinBtn.style.display = ''; }
      // 浏览模式：隐藏“立即清理”，仅保留“放心清理”作为主操作
      if (clExecBtn) { clExecBtn.style.display = 'none'; clExecBtn.disabled = false; clExecBtn.textContent = '立即清理'; }
      return;
    }
    if (clExecBtn) {
      clExecBtn.textContent = '清理所选';
      clExecBtn.style.display = '';
      clExecBtn.disabled = false;
    }
    if (cleanFangxinBtn) cleanFangxinBtn.style.display = 'none';
    if (clFloatingBar) clFloatingBar.style.display = '';
    renderCleanGroups();
    updateCleanHeader();
    if (clHeaderTitle) clHeaderTitle.textContent = '以下项目需要您确认后清理';
  }

  clExecBtn.addEventListener('click', runClean);

  if (cleanFangxinBtn) {
    cleanFangxinBtn.addEventListener('click', function () {
      cl.mode = 'fangxin';
      cl.phase = 1;
      cl.tab = 'all';
      cleanFangxinBtn.disabled = true;
      cleanFangxinBtn.style.display = 'none';
      if (clFloatingBar) clFloatingBar.style.display = 'none';
      if (cl.scanning) { api.cleanCancel(); }
      cl.scanning = true; cl.done = false; cl.groups = []; cl.path = '';
      clItemFilesCache = {};
      clGroups.innerHTML = '';
      clProgWrap.style.display = '';
      clProgFill.className = 'progress-fill indeterminate';
      clProgFill.style.width = '';
      if (clFloatingBar) clFloatingBar.style.display = 'none';
      if (clRescanBtn) clRescanBtn.style.display = 'none';
      if (clHeaderInfo) clHeaderInfo.style.display = 'none';
      var scanStatus = document.getElementById('cleanScanStatus');
      if (scanStatus) scanStatus.style.display = '';
      renderCleanGroups();
      beginCleanScan();
    });
  }

  function renderCleanGroups() {
    var totalSelected = calcSelectedSize(cl.groups.flatMap(function(g){return g.items||[];}));
    var h = '';
    if (cl.groups.length === 0 && cl.scanning) {
      for (var i = 0; i < 3; i++) {
        h += '<div class="clean-group"><div class="clean-group-header">';
        h += '<div class="placeholder-row" style="width:100%"><div class="placeholder-bar" style="width:50%;height:14px"></div></div>';
        h += '</div></div>';
      }
      clGroups.innerHTML = h;
      return;
    }
    if (cl.groups.length === 0 && !cl.scanning) {
      clGroups.innerHTML = '<p class="text-tertiary" style="text-align:center;padding:40px 0">\u672A\u53D1\u73B0\u9700\u8981\u5904\u7406\u7684\u9879</p>';
      return;
    }
    // 优先法则重排：①普通有结果分组 ②需确认(risky)分组 ③空分组折叠为"没有发现垃圾"区置底
    var withResults = [];
    var emptyGroups = [];
    var riskyGroups = [];
    cl.groups.forEach(function (g, gi) {
      if (g.risky) { riskyGroups.push({ g: g, gi: gi }); }
      else if (g.items && g.items.length > 0) { withResults.push({ g: g, gi: gi }); }
      else { emptyGroups.push({ g: g, gi: gi }); }
    });

    function renderGroup(g, gi) {
      var expanded = g._expanded !== false;
      var checkedCount = (g.items || []).filter(function(x){return x.checked;}).length;
      var totalCount = (g.items || []).length;
      var groupSelectedSize = calcSelectedSize(g.items);
      var iconKey = resolveGroupIcon(g);
      var hasResults = g.items && g.items.length > 0;
      var iconHtml = localSvg(iconKey, 20);
      var gh = '';
      gh += '<div class="clean-group' + (expanded ? ' expanded' : '') + (hasResults ? ' has-results' : ' empty') + '" data-gi="' + gi + '">';
      gh += '<div class="clean-group-header">';
      gh += '<label class="clean-group-toggle" onclick="event.stopPropagation()">';
      gh += '<input type="checkbox" ' + (g.checked ? 'checked' : '') + ' data-gi="' + gi + '" class="group-check">';
      gh += '<span class="checkbox-custom"></span></label>';
      gh += '<span class="clean-group-icon">' + iconHtml + '</span>';
      gh += '<span class="clean-group-name" title="' + esc(g.groupName) + '">' + esc(truncate(g.groupName, 20)) + '</span>';
      if (g.risky) gh += '<span class="clean-group-tag">需确认</span>';
      gh += '<div class="clean-group-meta">';
      if (hasResults) {
        if (groupSelectedSize > 0) {
          gh += '<span class="clean-group-selected-size">' + formatSize(groupSelectedSize) + '</span>';
        }
        gh += '<span class="clean-group-count">' + checkedCount + '/' + totalCount + '</span>';
      } else {
        gh += '<span class="clean-group-empty">\u6CA1\u6709\u53D1\u73B0\u5783\u573E</span>';
      }
      gh += '</div>';
      gh += '<svg class="clean-group-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><use href="#icon-chevron"/></svg>';
      gh += '</div>';
      gh += '<div class="clean-group-items"><div class="clean-group-items-inner">';
      var CLEAN_ITEM_PREVIEW = 5; // 预览模式: 默认仅渲染前5条, 其余由"+还有N款"展开
      (g.items || []).forEach(function (it, ii) {
        if (!g._showAll && ii >= CLEAN_ITEM_PREVIEW) return;
        // 子项使用软件原生图标（sprite），仅在无法找到时回退到本地SVG
        var iconCell = it.icon && ICON_MAP[it.icon] ? iconSvg(it.icon) : localSvg('folder', 16);
        gh += '<div class="clean-item' + (it.safe ? '' : ' unsafe') + (it.risky ? ' risky' : '') + '" data-gi="' + gi + '" data-ii="' + ii + '">';
        gh += '<label class="clean-item-check" onclick="event.stopPropagation()">';
        gh += '<input type="checkbox" ' + (it.checked ? 'checked' : '') + ' data-gi="' + gi + '" data-ii="' + ii + '" class="item-check">';
        gh += '<span class="checkbox-custom"></span></label>';
        gh += '<span class="clean-item-icon">' + iconCell + '</span>';
        gh += '<span class="clean-item-name" title="' + esc(it.name) + '">' + esc(truncate(it.name, 20)) + '</span>';
        gh += '<span class="clean-item-size">' + formatSize(it.size) + '</span>';
        gh += '<button class="clean-item-detail" type="button" data-gi="' + gi + '" data-ii="' + ii + '">\u8BE6\u60C5</button>';
        gh += '<button class="clean-item-folder" type="button" data-gi="' + gi + '" data-ii="' + ii + '" title="\u6253\u5F00\u6240\u5728\u6587\u4EF6\u5939">' + localSvg('folder', 16) + '</button>';
        gh += '</div>';
        gh += '<div class="clean-item-detail-panel" data-gi="' + gi + '" data-ii="' + ii + '" style="display:none;"></div>';
      });
      if (g.expandable && !g._showAll && (g.items || []).length > 5) {
        var restCount = (g.items || []).length - 5;
        gh += '<div class="clean-more"><span class="clean-more-icon">' + localSvg('list-bullet', 14) + '</span>+ \u8FD8\u6709 ' + restCount + ' \u6B3E\u5DF2\u652F\u6301\u6E05\u7406\u7684\u9879\u76EE</div>';
      }
      gh += '</div></div></div>';
      return gh;
    }

    withResults.forEach(function (pair) {
      h += renderGroup(pair.g, pair.gi);
    });

    if (riskyGroups.length > 0) {
      // 需确认(risky)分组: 紧随普通有结果分组之后, 排在"没有发现垃圾"折叠区上方
      riskyGroups.forEach(function (pair) {
        h += renderGroup(pair.g, pair.gi);
      });
    }

    if (emptyGroups.length > 0) {
      var emptyExpanded = cl._emptyExpanded === true;
      h += '<div class="clean-empty-wrap' + (emptyExpanded ? ' expanded' : '') + '">';
      h += '<div class="clean-empty-toggle">';
      h += '<svg class="clean-group-chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><use href="#icon-chevron"/></svg>';
      h += '<span class="clean-empty-toggle-label">\u6CA1\u6709\u53D1\u73B0\u5783\u573E (' + emptyGroups.length + ')</span>';
      h += '</div>';
      h += '<div class="clean-empty-body" style="' + (emptyExpanded ? '' : 'display:none;') + '">';
      emptyGroups.forEach(function (pair) {
        h += renderGroup(pair.g, pair.gi);
      });
      h += '</div>';
      h += '</div>';
    }
    clGroups.innerHTML = h;

    // Animate groups and items in with staggered delays (reactbits-style animated list)
    requestAnimationFrame(function () {
      var prefersReduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      if (prefersReduced) return;
      // 批量添加动画类，避免重复查询
      var groupEls = clGroups.children;
      for (var gi = 0; gi < groupEls.length; gi++) {
        var gEl = groupEls[gi];
        if (!gEl.classList.contains('clean-group')) continue;
        gEl.style.animationDelay = (gi * 60) + 'ms';
        gEl.classList.add('animating');
        var itemEls = gEl.children;
        for (var ii = 0; ii < itemEls.length; ii++) {
          var iEl = itemEls[ii];
          if (!iEl.classList.contains('clean-item')) continue;
          iEl.style.animationDelay = (gi * 60 + ii * 30) + 'ms';
          iEl.classList.add('animating');
        }
      }
    });

  // 更新浮动栏显示已选中的大小和数量
  if (clFloatingText) {
    var selCount = cl.groups.flatMap(function(g){return g.items||[];}).filter(function(x){return x.checked;}).length;
    clFloatingText.innerHTML = '已勾选：<strong>' + formatSize(totalSelected) + '</strong>（' + selCount + '项）';
  }
  if (clFloatingSubText) {
    clFloatingSubText.textContent = '清理后可释放约 ' + formatSize(totalSelected) + ' 磁盘空间';
  }

  updateCleanHeader();
}

// 事件委托：只绑定一次，通过 dataset 识别操作
function bindCleanEvents() {
  clGroups.addEventListener('click', function (ev) {
    var target = ev.target;
    // 空分组折叠区点击 → 展开/收起
    var et = target.closest('.clean-empty-toggle');
    if (et) {
      cl._emptyExpanded = !(cl._emptyExpanded === true);
      renderCleanGroups();
      return;
    }
    // 分组头点击 → 展开/折叠
    var gh = target.closest('.clean-group-header');
    if (gh && !target.closest('.clean-group-toggle')) {
      var gEl = gh.closest('.clean-group');
      var gi = parseInt(gEl.getAttribute('data-gi'));
      cl.groups[gi]._expanded = !cl.groups[gi]._expanded;
      renderCleanGroups();
      return;
    }
    // 分组全选 checkbox
    if (target.classList.contains('group-check')) {
      var gi2 = parseInt(target.getAttribute('data-gi'));
      cl.groups[gi2].checked = target.checked;
      cl.groups[gi2].items.forEach(function (it) { it.checked = target.checked; });
      renderCleanGroups();
      return;
    }
    // 单条 checkbox
    if (target.classList.contains('item-check')) {
      var gi3 = parseInt(target.getAttribute('data-gi'));
      var ii = parseInt(target.getAttribute('data-ii'));
      cl.groups[gi3].items[ii].checked = target.checked;
      cl.groups[gi3].checked = cl.groups[gi3].items.every(function(x){return x.checked;});
      renderCleanGroups();
      return;
    }
    // 详情按钮
    if (target.classList.contains('clean-item-detail')) {
      var gi4 = parseInt(target.getAttribute('data-gi'));
      var ii4 = parseInt(target.getAttribute('data-ii'));
      toggleItemDetail(gi4, ii4);
      return;
    }
    // 文件夹按钮 → 打开所在目录
    var folderBtn = target.closest('.clean-item-folder');
    if (folderBtn) {
      var fgi = parseInt(folderBtn.getAttribute('data-gi'));
      var fii = parseInt(folderBtn.getAttribute('data-ii'));
      var fItem = cl.groups[fgi] && cl.groups[fgi].items[fii];
      if (fItem && fItem.path) api.openFolder(fItem.path);
      return;
    }
    // 整行点击 → 展开/收起详情（避开复选框/详情按钮/文件夹按钮）
    var itemRow = target.closest('.clean-item');
    if (itemRow && !target.closest('.clean-item-check') && !target.closest('.item-check') && !target.closest('.clean-item-detail') && !target.closest('.clean-item-folder')) {
      var rgi = parseInt(itemRow.getAttribute('data-gi'));
      var rii = parseInt(itemRow.getAttribute('data-ii'));
      if (!isNaN(rgi) && !isNaN(rii)) { toggleItemDetail(rgi, rii); return; }
    }
    // 已支持清理的项目 → 展开/收起该分组
    var moreEl = target.closest('.clean-more');
    if (moreEl) {
      var mGroup = moreEl.closest('.clean-group');
      if (mGroup) {
        var mgi = parseInt(mGroup.getAttribute('data-gi'));
        if (!isNaN(mgi) && cl.groups[mgi]) {
          // "+还有N款" 语义: 展开剩余项(收起走组头点击), 不再误切整组折叠
          cl.groups[mgi]._showAll = true;
          renderCleanGroups();
        }
      }
      return;
    }
  });

  // 头部统计区点击 → 全部展开/折叠
  if (clHeaderInfo) {
    clHeaderInfo.style.cursor = 'pointer';
    clHeaderInfo.title = '点击展开/折叠所有分组';
    clHeaderInfo.addEventListener('click', function () {
      var allExpanded = cl.groups.every(function(g){ return g._expanded !== false; });
      cl.groups.forEach(function(g){ g._expanded = !allExpanded; });
      renderCleanGroups();
    });
  }
}

// 初始化事件绑定
bindCleanEvents();

// 仅在页面加载时添加一次 sidebar tooltip
(function initSidebarTooltips() {
  var links = document.querySelectorAll('.sidebar-item a');
  for (var i = 0; i < links.length; i++) {
    var link = links[i];
    var label = link.querySelector('.sidebar-label');
    if (label && !link.getAttribute('title')) {
      link.setAttribute('title', label.textContent.trim());
    }
  }
})();

  function toggleItemDetail(gi, ii) {
    var panel = clGroups.querySelector('.clean-item-detail-panel[data-gi="' + gi + '"][data-ii="' + ii + '"]');
    if (!panel) return;
    var isOpen = panel.style.display !== 'none';
    if (isOpen) {
      panel.style.display = 'none';
      return;
    }
    var g = cl.groups[gi];
    var it = g.items[ii];
    var cacheKey = g.groupId + ':' + it.id;
    if (clItemFilesCache[cacheKey]) {
      renderItemDetailPanel(panel, clItemFilesCache[cacheKey]);
    } else {
      panel.innerHTML = '<div class="clean-item-detail-loading">\u52A0\u8F7D\u4E2D...</div>';
      panel.style.display = '';
      api.cleanItemFiles(g.groupId, it.id).then(function (files) {
        var list = Array.isArray(files) ? files : [];
        clItemFilesCache[cacheKey] = list;
        renderItemDetailPanel(panel, list);
      }).catch(function () {
        panel.innerHTML = '<div class="clean-item-detail-loading">\u65E0\u6CD5\u83B7\u53D6\u8BE6\u60C5</div>';
      });
    }
  }

  function renderItemDetailPanel(panel, files) {
    panel.style.display = '';
    var h = '<div class="clean-item-detail-head">\u5171 ' + files.length + ' \u4E2A\u6587\u4EF6</div>';
    h += '<div class="clean-item-detail-list">';
    files.forEach(function (fp) {
      h += '<div class="clean-item-detail-file" title="' + esc(fp) + '">' + esc(fp) + '</div>';
    });
    h += '</div>';
    panel.innerHTML = h;
  }

  function updateCleanHeader() {
    if (!clHeaderInfo || cl.scanning) return;
    var allItems = cl.groups.flatMap(function(g){return g.items||[];});
    var totalSize = allItems.reduce(function(s,it){return s + (it.size||0);}, 0);
    var selSize = calcSelectedSize(allItems);
    var selCount = allItems.filter(function(x){return x.checked;}).length;
    var totalCount = allItems.length;
    var unselCount = totalCount - selCount;
    if (cl.groups.length === 0) return;
    clHeaderInfo.style.display = '';
    if (clRescanBtn) clRescanBtn.style.display = '';
    var scanStatus = document.getElementById('cleanScanStatus');
    if (scanStatus) scanStatus.style.display = 'none';
    if (clHeaderTitle) clHeaderTitle.textContent = '\u626B\u63CF\u5B8C\u6210\uFF01';
    if (clHeaderSize) {
      clHeaderSize.textContent = '\u5171\u53D1\u73B0 ' + formatSize(totalSize) + ' \u5783\u573E\uFF0C\u5DF2\u52FE\u9009 ' + formatSize(selSize) + '\uFF08' + selCount + '\u9879\uFF09';
    }
    if (clHeaderStatus) {
      var hint = '\u6E05\u7406\u540E\u7ACB\u5373\u91CA\u653E\u78C1\u76D8\u53EF\u7528\u7A7A\u95F4';
      if (unselCount > 0) hint = '\uFF0C\u8FD8\u6709 ' + unselCount + ' \u9879\u672A\u52FE\u9009\u00B7' + hint;
      clHeaderStatus.textContent = hint;
    }
  }

  subscribe('clean:progress', function (p) {
    cl.path = p.currentPath || '';
    clPath.textContent = '\u6B63\u5728\u626B\u63CF\uFF1A' + cl.path;
    if (p.percent != null) {
      clProgFill.className = 'progress-fill';
      clProgFill.style.width = p.percent + '%';
    }
  });

  subscribe('clean:group-status', function (p) {
    // 只接受当前扫描会话的事件（scanKey 由主进程统一发号）
    if (p.scanKey !== cl.scanKey) return;
    // 扫描中不更新 DOM，只缓存数据，避免中间状态闪烁
    var ex = cl.groups.filter(function(x){return x.groupId===p.groupId;})[0];
    if (ex) {
      ex.items = p.found || [];
      // 勾选态以主进程为准（risky 分组默认不勾选）
      ex.checked = p.checked !== undefined ? !!p.checked : ex.checked;
      ex.items.forEach(function(it){ if(it.checked===undefined) it.checked=ex.checked; });
    } else if (p.groupId) {
      var ng = {
        groupId: p.groupId, groupName: p.groupName || p.groupId, icon: p.icon || 'folder',
        category: cl.tab, checked: !!p.checked, items: p.found || [],
        expandable: true, extraCount: (p.found || []).length, _expanded: false,
      };
      ng.items.forEach(function(it){ it.checked = ng.checked; });
      cl.groups.push(ng);
    }
    // 只在扫描进行中时不渲染，等待 done 回调统一渲染
    if (!cl.scanning) renderCleanGroups();
  });

  subscribe('clean:done', function (p) {
    // 只接受当前扫描会话的完成事件（scanKey 由主进程统一发号）
    if (p.scanKey !== cl.scanKey) return;
    cl.scanning = false; cl.done = true;
    clProgWrap.style.display = 'none';
    if (p.groups) {
      cl.groups = p.groups;
      cl.cache[cl.tab] = { groups: p.groups };
    } else {
      cl.cache[cl.tab] = { groups: cl.groups.slice() };
    }
    cl.groups.forEach(function(g){
      if(g._expanded===undefined) g._expanded=false;
      // 勾选态以主进程为准：risky 分组默认不勾选
      (g.items||[]).forEach(function(it){ if(it.checked===undefined) it.checked=!!g.checked; });
    });
    if (cl.mode === 'fangxin' && cl.phase === 1) {
      // 放心清理第一阶段：仅勾选安全项，随后自动清理
      cl.groups.forEach(function(g){
        var risky = !!g.risky;
        g.checked = !risky;
        (g.items||[]).forEach(function(it){ it.checked = !risky; });
      });
      renderCleanGroups();
      updateCleanHeader();
      if (clFloatingBar) clFloatingBar.style.display = '';
      runClean();
      return;
    }
    // Show floating bar after scan
    if (clFloatingBar) clFloatingBar.style.display = '';
    // 渲染最终结果
    renderCleanGroups();
  });

  subscribe('clean:exec-progress', function (p) {
    clExecBtn.textContent = '\u6E05\u7406\u4E2D ' + p.current + '/' + p.total;
  });

  /* ===========================================================
     STARTUP PAGE
     =========================================================== */
  var st = { tab: 'software', items: [], hideDisabled: false, loading: false, search: '', selected: new Set() };
  var stTabs = document.getElementById('startupTabs');
  var stList = document.getElementById('startupList');
  var stEmpty = document.getElementById('startupEmpty');
  var stWrap = document.querySelector('.startup-table-wrap');
  var hideCheck = document.getElementById('hideDisabled');
  var smartBtn = document.getElementById('smartOptimizeBtn');
  var stFloatingBar = document.getElementById('startupFloatingBar');
  var stFloatingText = document.getElementById('startupFloatingText');
  var stLoading = document.getElementById('startupLoading');
  // F1/F2/F3/F4/F5/F6 新增控件引用
  var searchInput = document.getElementById('startupSearch');
  var checkAll = document.getElementById('startupCheckAll');
  var stBatchBar = document.getElementById('startupBatchBar');
  var stBatchText = document.getElementById('startupBatchText');
  var batchDisableBtn = document.getElementById('batchDisableBtn');
  var batchEnableBtn = document.getElementById('batchEnableBtn');
  var batchRemoveBtn = document.getElementById('batchRemoveBtn');
  var batchClearBtn = document.getElementById('batchClearBtn');
  var startupBackupBtn = document.getElementById('startupBackupBtn');
  var startupRestoreBtn = document.getElementById('startupRestoreBtn');
  var startupAddBtn = document.getElementById('startupAddBtn');
  var detailModal = document.getElementById('startupDetailModal');
  var detailBody = document.getElementById('startupDetailBody');
  var addModal = document.getElementById('startupAddModal');
  var addName = document.getElementById('addName');
  var addCommand = document.getElementById('addCommand');
  var addLocation = document.getElementById('addLocation');
  var addHint = document.getElementById('addHint');
  var addConfirmBtn = document.getElementById('addConfirmBtn');
  var backupModal = document.getElementById('startupBackupModal');
  var backupListEl = document.getElementById('startupBackupList');

  // 代际守卫: 快速切换标签时, 迟到的旧响应不得覆盖新状态 (对齐 cleanScan 的 scanKey 失效语义)
  var stLoadSeq = 0;
  function loadStartupTab() {
    var seq = ++stLoadSeq;
    st.tab = (stTabs.querySelector('.tab.active') || {}).getAttribute('data-tab') || 'software';
    st.items = [];
    st.loading = true;
    renderStartupList();
    api.startupList(st.tab).then(function (items) {
      if (seq !== stLoadSeq) return; // 已被更新的点击取代, 丢弃过期响应
      st.items = items || [];
      st.loading = false;
      renderStartupList();
    }).catch(function () {
      if (seq !== stLoadSeq) return;
      st.loading = false;
      renderStartupList();
    });
  }

  stTabs.querySelectorAll('.tab').forEach(function (t) {
    t.addEventListener('click', function () {
      stTabs.querySelector('.tab.active').classList.remove('active');
      this.classList.add('active');
      st.tab = this.getAttribute('data-tab');
      positionTabIndicator(stTabs);
      loadStartupTab();
    });
  });

  hideCheck.addEventListener('change', function () {
    st.hideDisabled = this.checked;
    renderStartupList();
  });

  smartBtn.addEventListener('click', function () {
    smartBtn.disabled = true;
    smartBtn.textContent = '\u4F18\u5316\u4E2D...';
    api.startupSmartOptimize().then(function (r) {
      smartBtn.disabled = false;
      smartBtn.textContent = '\u4E00\u952E\u667A\u80FD\u4F18\u5316';
      if (r && r.ok) loadStartupTab();
    }).catch(function () {
      smartBtn.disabled = false;
      smartBtn.textContent = '\u4E00\u952E\u667A\u80FD\u4F18\u5316';
    });
  });

  // 全局回到顶部：优先滚动当前视图内的子级滚动容器（如启动项表格），否则滚动 .main-content
  (function () {
    var backToTopBtn = document.getElementById('backToTopBtn');
    if (!backToTopBtn) return;
    backToTopBtn.addEventListener('click', function () {
      var active = document.querySelector('.view.active');
      var scrollEl = active ? active.querySelector('.startup-table-wrap') : null;
      if (!scrollEl) scrollEl = document.querySelector('.main-content');
      if (scrollEl) scrollEl.scrollTo({ top: 0, behavior: 'smooth' });
    });
  })();

  /* ---- per-item exe icon loading via getFileIcon IPC ---- */
  var _iconCache = new Map();
  function _loadStartupIcons() {
    var icons = stList ? stList.querySelectorAll('.startup-item-icon[data-id]') : [];
    if (!icons.length) return;
    // 建立 id -> item 的 Map 索引，避免 O(n²) 线性查找
    var itemById = new Map();
    for (var k = 0; k < st.items.length; k++) {
      itemById.set(String(st.items[k].id), st.items[k]);
    }
    icons.forEach(function (cell) {
      var it = itemById.get(cell.getAttribute('data-id'));
      if (!it || !it.target) return;
      if (!/\.(exe|dll|com|bat|cmd|lnk|ico)$/i.test(it.target)) return;
      var key = it.target.toLowerCase();
      if (_iconCache.has(key)) {
        _applyIcon(cell, _iconCache.get(key));
        return;
      }
      api.getFileIcon(it.target).then(function (dataUrl) {
        if (dataUrl) {
          _iconCache.set(key, dataUrl);
          _applyIcon(cell, dataUrl);
        }
      }).catch(function () {
        /* keep placeholder SVG on failure */
      });
    });
  }
  function _applyIcon(cell, dataUrl) {
    cell.innerHTML = '<img class="startup-item-img" src="' + dataUrl + '" alt="">';
  }

  function renderStartupList() {
    if (st.loading) {
      if (stWrap) stWrap.style.display = 'none';
      if (stEmpty) stEmpty.style.display = 'none';
      if (stLoading) stLoading.style.display = '';
      if (stFloatingBar) stFloatingBar.style.display = 'none';
      if (stBatchBar) stBatchBar.style.display = 'none';
      return;
    }
    if (stLoading) stLoading.style.display = 'none';
    var items = st.items;
    // F4 搜索筛选（名称 / 描述 / 路径）
    var q = (st.search || '').trim().toLowerCase();
    if (q) {
      items = items.filter(function (it) {
        return (it.name || '').toLowerCase().indexOf(q) >= 0 ||
               (it.desc || '').toLowerCase().indexOf(q) >= 0 ||
               (it.target || '').toLowerCase().indexOf(q) >= 0;
      });
    }
    if (st.hideDisabled) {
      items = items.filter(function (it) { return it.enabled; });
    }
    if (items.length === 0) {
      stWrap.style.display = 'none';
      stEmpty.style.display = '';
      if (stFloatingBar) stFloatingBar.style.display = 'none';
      if (stBatchBar) stBatchBar.style.display = 'none';
      return;
    }
    stWrap.style.display = '';
    stEmpty.style.display = 'none';

    var h = '';
    items.forEach(function (it, i) {
      h += '<tr' + (it.ignored ? ' class="startup-row-ignored"' : '') + '>';
      h += '<td class="startup-check"><input type="checkbox" class="startup-row-check" data-id="' + esc(it.id) + '"' + (st.selected.has(it.id) ? ' checked' : '') + '></td>';
      h += '<td><div class="startup-name-cell">';
      h += '<div class="startup-item-icon" data-id="' + esc(it.id) + '">' + iconSvg(it.icon || 'app') + '</div>';
      h += '<div class="startup-item-text">';
      h += '<div class="startup-item-name" title="' + esc(it.name) + '">' + esc(truncate(it.name, 30));
      if (it.ignored) h += '<span class="startup-ignored-badge">已忽略</span>';
      h += '</div>';
      if (it.desc) h += '<div class="startup-item-desc">' + esc(it.desc) + '</div>';
      h += '</div></div></td>';
      h += '<td class="startup-ban">' + (it.banRate != null ? it.banRate + '%' : '-') + '</td>';
      h += '<td class="startup-time">' + formatStartupTime(it.startupTime) + '</td>';
      var sc = 'suggest-keep';
      if (it.suggestion === '\u5EFA\u8BAE\u7981\u7528') sc = 'suggest-disable';
      else if (it.suggestion === '\u5EFA\u8BAE\u5F00\u542F') sc = 'suggest-enable';
      h += '<td class="startup-suggest ' + sc + '">' + esc(it.suggestion || '') + '</td>';
      h += '<td class="startup-status-cell"><span class="startup-status-badge ' + (it.enabled ? 'status-on' : 'status-off') + '">' + (it.enabled ? '\u5DF2\u5F00\u542F' : '\u5DF2\u7981\u7528') + '</span></td>';
      h += '<td><div class="startup-actions">';
      if (it.canToggle !== false) {
        h += '<button class="startup-toggle-btn ' + (it.enabled ? 'btn-disable' : 'btn-enable') + '" data-id="' + it.id + '" data-enabled="' + (it.enabled ? '1' : '0') + '">';
        h += (it.enabled ? '\u7981\u6B62\u542F\u52A8' : '\u6062\u590D\u542F\u52A8') + '</button>';
      }
      if (it.settings && it.settings.length > 0) {
        h += '<button class="settings-btn" data-id="' + it.id + '" title="\u8BBE\u7F6E">';
        h += iconSvg('settings');
        h += '</button>';
      }
      h += '</div></td>';
      h += '</tr>';
    });
    stList.innerHTML = h;

    _loadStartupIcons();

    _updateBars();

    stList.querySelectorAll('.startup-toggle-btn').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var id = this.getAttribute('data-id');
        var enabled = this.getAttribute('data-enabled') === '1';
        btn.disabled = true;
        btn.textContent = '\u5904\u7406\u4E2D...';
        api.startupToggle(id, !enabled).then(function (r) {
          btn.disabled = false;
          if (r && r.ok) {
            loadStartupTab();
          } else {
            // 还原按钮文案
            btn.textContent = enabled ? '\u7981\u6B62\u542F\u52A8' : '\u6062\u590D\u542F\u52A8';
          }
        }).catch(function () {
          btn.disabled = false;
          btn.textContent = enabled ? '\u7981\u6B62\u542F\u52A8' : '\u6062\u590D\u542F\u52A8';
        });
      });
    });

    stList.querySelectorAll('.settings-btn').forEach(function (btn) {
      btn.addEventListener('click', function (e) {
        e.stopPropagation();
        var id = this.getAttribute('data-id');
        var item = st.items.filter(function(x){return x.id===id;})[0];
        openStartupMenu(btn, item);
      });
    });

    stList.querySelectorAll('.startup-row-check').forEach(function (cb) {
      cb.addEventListener('change', function () {
        var id = cb.getAttribute('data-id');
        if (cb.checked) st.selected.add(id); else st.selected.delete(id);
        _updateBars();
        _syncCheckAll();
      });
    });
  }

  function openStartupMenu(anchor, item) {
    closeStartupMenu();
    var menu = document.createElement('div');
    menu.className = 'startup-menu';
    var items = [
      { label: '打开所在目录', icon: 'folder', action: 'open' },
      { label: '查看详情', icon: 'info', action: 'detail' },
      { label: item.ignored ? '取消忽略' : '忽略（加入信任白名单）', icon: 'shield-check', action: 'ignore' },
      { label: '删除启动项', icon: 'trash', action: 'remove', danger: true },
    ];
    var h = '';
    items.forEach(function (mi) {
      h += '<button class="startup-menu-item' + (mi.danger ? ' danger' : '') + '" data-action="' + mi.action + '" data-id="' + esc(item.id) + '">';
      h += '<span class="startup-menu-icon">' + localSvg(mi.icon, 15) + '</span>' + mi.label + '</button>';
    });
    menu.innerHTML = h;
    document.body.appendChild(menu);

    var rect = anchor.getBoundingClientRect();
    menu.style.position = 'fixed';
    menu.style.left = (rect.right - 150) + 'px';
    menu.style.top = (rect.bottom + 4) + 'px';
    menu.setAttribute('data-startup-menu', '1');

    menu.querySelectorAll('.startup-menu-item').forEach(function (mi) {
      mi.addEventListener('click', function () {
        var action = mi.getAttribute('data-action');
        var itemId = mi.getAttribute('data-id');
        handleStartupAction(action, itemId);
        closeStartupMenu();
      });
    });
  }

  function closeStartupMenu() {
    var m = document.querySelector('.startup-menu');
    if (m) m.remove();
  }

  function handleStartupAction(action, itemId) {
    if (action === 'open') {
      api.startupOpenLocation(itemId).then(function (r) {
        if (r && !r.ok && r.message) showToast(r.message);
      });
    } else if (action === 'remove') {
      if (!confirm('确定删除该启动项吗？此操作无法撤销。')) return;
      api.startupRemove(itemId).then(function (r) {
        if (r && r.ok) {
          showToast('已删除');
        } else if (r && r.message) {
          showToast(r.message);
        }
      }).catch(function (e) {
        showToast('删除失败: ' + (e && e.message || '未知错误'));
      }).then(function () {
        loadStartupTab();
      });
    } else if (action === 'detail') {
      api.startupDetail(itemId).then(function (r) {
        if (r && r.ok && r.detail) {
          openStartupDetail(r.detail);
        } else {
          showToast((r && r.message) || '无详细信息');
        }
      });
    } else if (action === 'ignore') {
      var it = st.items.filter(function (x) { return x.id === itemId; })[0];
      if (!it) return;
      api.startupSetIgnored(itemId, !it.ignored).then(function (r) {
        if (r && r.ok) {
          showToast(it.ignored ? '已取消忽略' : '已加入信任白名单');
          loadStartupTab();
        } else if (r && r.message) {
          showToast(r.message);
        }
      });
    }
  }

  // 关闭菜单：点击空白处
  document.addEventListener('click', function (e) {
    if (!e.target.closest('.startup-menu') && !e.target.closest('.settings-btn')) {
      closeStartupMenu();
    }
  });

  // ============== F1/F2/F3/F4/F5/F6 新增逻辑 ==============

  function openStartupDetail(d) {
    if (!detailModal) return;
    var rows = [];
    function row(k, v) {
      if (v == null || v === '') return;
      rows.push('<div class="detail-row"><span class="detail-key">' + esc(k) + '</span><span class="detail-val">' + esc(String(v)) + '</span></div>');
    }
    row('名称', d.name);
    row('建议', d.suggestion);
    row('当前状态', d.enabled ? '已开启' : '已禁用');
    row('命令', d.command);
    if (d.runKey) row('注册表位置', d.runKey + '\\' + (d.valueName || ''));
    row('计划任务', d.taskName);
    row('快捷方式', d.lnkPath);
    row('服务名', d.serviceName);
    row('COM 组件', d.clsid);
    row('说明', d.desc);
    if (d.essential) rows.push('<div class="detail-row detail-essential"><span class="detail-val">\u26A0 系统必需组件，谨慎禁用</span></div>');
    if (d.startupTime != null) row('启动用时', formatStartupTime(d.startupTime));
    detailBody.innerHTML = rows.length ? rows.join('') : '<div class="detail-empty">无详细信息</div>';
    detailModal.style.display = '';
  }

  function closeModal(el) {
    if (el) el.style.display = 'none';
  }

  function _updateBars() {
    _updateBatchBar();
    _syncCheckAll();
    _refreshStartupFloatingBar();
  }

  function _updateBatchBar() {
    if (!stBatchBar) return;
    var n = st.selected.size;
    if (n === 0) {
      stBatchBar.style.display = 'none';
      return;
    }
    stBatchBar.style.display = '';
    if (stBatchText) stBatchText.textContent = '已选 ' + n + ' 项';
  }

  function _syncCheckAll() {
    if (!checkAll) return;
    var total = st.items.length;
    var sel = 0;
    st.items.forEach(function (it) { if (st.selected.has(it.id)) sel++; });
    checkAll.checked = total > 0 && sel === total;
    checkAll.indeterminate = sel > 0 && sel < total;
  }

  function _batchAct(enabled, action) {
    var ids = Array.from(st.selected);
    if (!ids.length) return;
    if (action === 'remove') {
      if (!confirm('确定删除选中的 ' + ids.length + ' 个启动项吗？此操作无法撤销。')) return;
    }
    var pending = ids.length;
    var done = 0;
    var okCount = 0;
    ids.forEach(function (id) {
      var p = action === 'remove' ? api.startupRemove(id) : api.startupToggle(id, enabled);
      p.then(function (r) {
        if (r && r.ok) okCount++;
        else if (r && r.message) showToast(r.message);
      }).catch(function () {
        /* 单个失败不影响其余 */
      }).then(function () {
        done++;
        if (done === pending) {
          showToast((action === 'remove' ? '已删除 ' : (enabled ? '已恢复 ' : '已禁用 ')) + okCount + ' 项');
          loadStartupTab();
        }
      });
    });
  }

  // ---- F4 搜索筛选 ----
  if (searchInput) {
    searchInput.addEventListener('input', function () {
      st.search = this.value || '';
      renderStartupList();
    });
  }

  // ---- F5 全选 / 取消全选 ----
  if (checkAll) {
    checkAll.addEventListener('change', function () {
      var checked = this.checked;
      st.items.forEach(function (it) {
        if (checked) st.selected.add(it.id); else st.selected.delete(it.id);
      });
      renderStartupList();
      _syncCheckAll();
    });
  }

  // ---- F5 批量操作 ----
  if (batchEnableBtn) batchEnableBtn.addEventListener('click', function () { _batchAct(true, 'toggle'); });
  if (batchDisableBtn) batchDisableBtn.addEventListener('click', function () { _batchAct(false, 'toggle'); });
  if (batchRemoveBtn) batchRemoveBtn.addEventListener('click', function () { _batchAct(false, 'remove'); });
  if (batchClearBtn) batchClearBtn.addEventListener('click', function () {
    st.selected.clear();
    renderStartupList();
    _syncCheckAll();
  });

  // ---- F6 详情弹窗关闭 ----
  if (detailModal) {
    detailModal.addEventListener('click', function (e) {
      if (e.target === detailModal) closeModal(detailModal);
    });
    var detailClose = detailModal.querySelector('.modal-close');
    if (detailClose) detailClose.addEventListener('click', function () { closeModal(detailModal); });
  }

  // ---- F1 添加启动项 ----
  function openAddModal() {
    if (!addModal) return;
    addName.value = '';
    addCommand.value = '';
    addLocation.value = 'hkcu_run';
    addHint.textContent = '';
    addModal.style.display = '';
    addName.focus();
  }
  if (startupAddBtn) startupAddBtn.addEventListener('click', openAddModal);
  if (addModal) {
    addModal.addEventListener('click', function (e) {
      if (e.target === addModal) closeModal(addModal);
    });
    var addClose = addModal.querySelector('.modal-close');
    if (addClose) addClose.addEventListener('click', function () { closeModal(addModal); });
  }
  if (addConfirmBtn) {
    addConfirmBtn.addEventListener('click', function () {
      var name = (addName.value || '').trim();
      var command = (addCommand.value || '').trim();
      if (!name || !command) {
        addHint.textContent = '名称和命令不能为空';
        return;
      }
      addConfirmBtn.disabled = true;
      api.startupAdd({ name: name, command: command, location: addLocation.value }).then(function (r) {
        addConfirmBtn.disabled = false;
        if (r && r.ok) {
          closeModal(addModal);
          showToast('已添加启动项');
          loadStartupTab();
        } else {
          addHint.textContent = (r && r.message) || '添加失败';
        }
      }).catch(function () {
        addConfirmBtn.disabled = false;
        addHint.textContent = '添加失败';
      });
    });
  }

  // ---- F2 备份 / 恢复 ----
  if (startupBackupBtn) {
    startupBackupBtn.addEventListener('click', function () {
      startupBackupBtn.disabled = true;
      api.startupBackup().then(function (r) {
        startupBackupBtn.disabled = false;
        if (r && r.ok) showToast('已备份 ' + r.count + ' 个启动项');
        else if (r && r.message) showToast(r.message);
      }).catch(function () { startupBackupBtn.disabled = false; });
    });
  }
  if (startupRestoreBtn) {
    startupRestoreBtn.addEventListener('click', function () {
      api.startupListBackups().then(function (list) {
        if (!backupModal) return;
        var arr = list || [];
        if (!arr.length) {
          backupListEl.innerHTML = '<div class="backup-empty">暂无备份</div>';
        } else {
          backupListEl.innerHTML = arr.map(function (b) {
            var t = b.createdAt ? new Date(b.createdAt).toLocaleString() : '';
            return '<div class="backup-item">' +
              '<div class="backup-meta"><span>' + esc(b.file) + '</span><span>' + esc(t) + ' · ' + b.count + ' 项</span></div>' +
              '<button class="btn-ghost backup-restore-btn" data-file="' + esc(b.file) + '">恢复</button>' +
              '</div>';
          }).join('');
          backupListEl.querySelectorAll('.backup-restore-btn').forEach(function (btn) {
            btn.addEventListener('click', function () {
              var file = btn.getAttribute('data-file');
              btn.disabled = true;
              api.startupRestore(file).then(function (r) {
                btn.disabled = false;
                if (r && r.ok) {
                  closeModal(backupModal);
                  showToast('已恢复 ' + r.applied + ' 项' + (r.skipped ? '，跳过 ' + r.skipped + ' 项' : ''));
                  loadStartupTab();
                } else if (r && r.message) {
                  showToast(r.message);
                }
              }).catch(function () { btn.disabled = false; });
            });
          });
        }
        backupModal.style.display = '';
      }).catch(function () { /* 忽略 */ });
    });
  }
  if (backupModal) {
    backupModal.addEventListener('click', function (e) {
      if (e.target === backupModal) closeModal(backupModal);
    });
    var backupClose = backupModal.querySelector('.modal-close');
    if (backupClose) backupClose.addEventListener('click', function () { closeModal(backupModal); });
  }

  function _refreshStartupFloatingBar() {
    if (!stFloatingBar) return;
    var optCount = st.items.filter(function (it) {
      return it.enabled && it.suggestion === '\u5EFA\u8BAE\u7981\u7528';
    }).length;
    stFloatingBar.style.display = '';
    if (stFloatingText) stFloatingText.textContent = '\u53EF\u4F18\u5316 ' + optCount + ' \u4E2A\u542F\u52A8\u9879';
    smartBtn.disabled = false;
  }

  /* ===========================================================
     SHREDDER PAGE
     =========================================================== */
  var sr = { queue: [], method: 'dod', running: false };
  var srDropZone = document.getElementById('shredderDropZone');
  var srQueue = document.getElementById('shredderQueue');
  var srEmpty = document.getElementById('shredderEmpty');
  var srActions = document.getElementById('shredderActions');
  var srProgress = document.getElementById('shredderProgress');
  var srProgressFill = document.getElementById('shredderProgressFill');
  var srProgressText = document.getElementById('shredderProgressText');
  var srResults = document.getElementById('shredderResults');
  var srShredBtn = document.getElementById('shredderShredBtn');
  var srCancelBtn = document.getElementById('shredderCancelBtn');
  var srClearBtn = document.getElementById('shredderClearBtn');
  var srMethod = document.getElementById('shredderMethod');
  var srPasses = document.getElementById('shredderPasses');

  function renderShredderQueue() {
    if (!srQueue) return;
    if (sr.queue.length === 0) {
      srEmpty.style.display = '';
      srActions.style.display = 'none';
      srQueue.innerHTML = '';
      srQueue.appendChild(srEmpty);
      return;
    }
    srEmpty.style.display = 'none';
    srActions.style.display = '';
    var h = '';
    sr.queue.forEach(function (item, idx) {
      h += '<div class="shredder-queue-item" data-idx="' + idx + '">';
      h += '  <span class="shredder-queue-name">' + esc(item.name) + '</span>';
      h += '  <span class="shredder-queue-size">' + formatSize(item.size) + '</span>';
      h += '  <span class="shredder-queue-status" id="sr-status-' + idx + '">' + (item.status || '\u5F85\u5904\u7406') + '</span>';
      h += '  <button class="shredder-queue-remove" data-idx="' + idx + '" title="\u79FB\u9664">\u2715</button>';
      h += '</div>';
    });
    srQueue.innerHTML = h;
    srQueue.querySelectorAll('.shredder-queue-remove').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var i = parseInt(btn.getAttribute('data-idx'));
        sr.queue.splice(i, 1);
        renderShredderQueue();
      });
    });
  }

  async function addFileToQueue(filePath) {
    try {
      var r = await api.shredderStatFile(filePath);
      if (r && !r.error && !r.isDirectory) {
        sr.queue.push({ path: filePath, name: r.name, size: r.size, status: '\u5F85\u5904\u7406' });
        renderShredderQueue();
      }
    } catch (e) {}
  }

  async function addFolderToQueue(folderPath) {
    try {
      var r = await api.shredderBrowseFolder(folderPath);
      if (r && r.files) {
        r.files.forEach(function (f) {
          sr.queue.push({ path: f.path, name: f.name, size: f.size, status: '\u5F85\u5904\u7406' });
        });
        renderShredderQueue();
      }
    } catch (e) {}
  }

  async function startShredding() {
    if (sr.running || sr.queue.length === 0) return;
    sr.running = true;
    srShredBtn.style.display = 'none';
    srCancelBtn.style.display = '';
    srProgress.style.display = '';
    srResults.style.display = 'none';

    var results = [];
    var totalSize = sr.queue.reduce(function (s, it) { return s + it.size; }, 0);
    var doneSize = 0;

    for (var i = 0; i < sr.queue.length; i++) {
      var item = sr.queue[i];
      var statusEl = document.getElementById('sr-status-' + i);
      if (statusEl) statusEl.textContent = '\u5904\u7406\u4E2D...';

      var isFolder = false;
      try { isFolder = item.isDirectory; } catch {}

      var r;
      if (isFolder) {
        r = await api.shredFolder(item.path, sr.method);
      } else {
        r = await api.shredFile(item.path, sr.method);
      }

      item.status = r.ok ? '\u6210\u529F' : '\u5931\u8D25';
      if (statusEl) statusEl.textContent = r.ok ? '\u6210\u529F' : '\u5931\u8D25';
      if (r.ok) doneSize += item.size;
      results.push(r);

      var pct = totalSize > 0 ? Math.round((doneSize / totalSize) * 100) : 0;
      if (srProgressFill) srProgressFill.style.width = pct + '%';
      if (srProgressText) srProgressText.textContent = pct + '%';
    }

    sr.running = false;
    srCancelBtn.style.display = 'none';
    srShredBtn.style.display = '';

    // Show results
    var okCount = results.filter(function (r) { return r.ok; }).length;
    var failCount = results.length - okCount;
    var msg = '\u5DF2\u5904\u7406 ' + results.length + ' \u4E2A\uFF0C\u6210\u529F ' + okCount + '\uFF0C\u5931\u8D25 ' + failCount;
    srResults.innerHTML = '<p class="text-secondary">' + msg + '</p>';
    srResults.style.display = '';

    sr.queue = [];
    renderShredderQueue();
  }

  if (srDropZone) {
    srDropZone.addEventListener('dragover', function (e) { e.preventDefault(); srDropZone.classList.add('drag-over'); });
    srDropZone.addEventListener('dragleave', function () { srDropZone.classList.remove('drag-over'); });
    srDropZone.addEventListener('drop', function (e) {
      e.preventDefault();
      srDropZone.classList.remove('drag-over');
      var files = e.dataTransfer.files;
      var paths = [];
      for (var i = 0; i < files.length; i++) {
        if (files[i].path) paths.push(files[i].path);
      }
      paths.forEach(addFileToQueue);
    });
  }

  var srAddFileBtn = document.getElementById('shredderAddFile');
  var srAddFolderBtn = document.getElementById('shredderAddFolder');
  if (srAddFileBtn) srAddFileBtn.addEventListener('click', async function () {
    var r = await api.shredderOpenFile();
    if (r && r.paths) r.paths.forEach(addFileToQueue);
  });
  if (srAddFolderBtn) srAddFolderBtn.addEventListener('click', async function () {
    var r = await api.shredderOpenFolder();
    if (r && r.path) addFolderToQueue(r.path);
  });

  if (srShredBtn) srShredBtn.addEventListener('click', startShredding);
  if (srCancelBtn) srCancelBtn.addEventListener('click', function () {
    if (api.shredCancel) api.shredCancel();
    sr.running = false;
    srCancelBtn.style.display = 'none';
    srShredBtn.style.display = '';
  });
  if (srClearBtn) srClearBtn.addEventListener('click', function () {
    sr.queue = [];
    renderShredderQueue();
  });
  if (srMethod) srMethod.addEventListener('change', function () { sr.method = this.value; });

  // Subscribe to progress events from main process
  subscribe('shredder:progress', function (p) {
    if (!sr.running) return;
    if (srProgressFill) srProgressFill.style.width = (p.percent || 0) + '%';
    if (srProgressText) srProgressText.textContent = (p.percent || 0) + '%';
  });

  /* ===========================================================
     SETTINGS PAGE
     =========================================================== */
  (function () {
    var setAutostart = document.getElementById('setAutostart');
    var setConfirmClose = document.getElementById('setConfirmClose');
    var setClearStats = document.getElementById('setClearStats');
    if (!setAutostart) return;

    /* ── Theme UI wiring ── */
    function highlightSwatch(id) {
      document.querySelectorAll('.theme-swatch').forEach(function (el) {
        el.classList.toggle('active', el.dataset.theme === id);
      });
    }
    function loadThemeUI() {
      Promise.resolve(api.storeGet('theme'))
        .then(function (id) { highlightSwatch(id || 'sky'); })
        .catch(function () { highlightSwatch('sky'); });
    }
    var swatchWrap = document.getElementById('themeSwatches');
    if (swatchWrap) {
      swatchWrap.addEventListener('click', function (e) {
        var btn = e.target.closest('.theme-swatch');
        if (!btn) return;
        var id = btn.dataset.theme;
        applyTheme(id);          // global function
        highlightSwatch(id);
        Promise.resolve(api.storeSet('theme', id)).catch(function () {});
      });
    }
    loadThemeUI();

    function refreshSettings() {
      Promise.resolve(api.settingsGetAutostart ? api.settingsGetAutostart() : false)
        .then(function (on) { setAutostart.checked = !!on; })
        .catch(function () { setAutostart.checked = false; });
      Promise.resolve(api.storeGet('confirmClose'))
        .then(function (v) { setConfirmClose.checked = !!v; })
        .catch(function () {});
    }

    setAutostart.addEventListener('change', function () {
      var on = this.checked;
      if (api.settingsSetAutostart) {
        Promise.resolve(api.settingsSetAutostart(on)).then(function (r) {
          if (!r || !r.ok) setAutostart.checked = !on; // revert on failure
        }).catch(function () { setAutostart.checked = !on; });
      }
    });

    setConfirmClose.addEventListener('change', function () {
      Promise.resolve(api.storeSet('confirmClose', this.checked)).catch(function () {});
    });

    if (setClearStats) {
      setClearStats.addEventListener('click', function () {
        if (!window.confirm('确定要清除所有使用统计数据吗？此操作不可撤销。')) return;
        Promise.all([
          api.storeSet('history', []),
          api.storeSet('totalCleanedMB', 0),
          api.storeSet('cleanCount', 0),
        ]).then(function () {
          setClearStats.textContent = '已清除';
          setTimeout(function () { setClearStats.textContent = '清除'; }, 1500);
        }).catch(function () {});
      });
    }

    var githubLink = document.getElementById('authorGithub');
    var feedback = document.getElementById('authorFeedback');
    var update = document.getElementById('authorUpdate');
    function openGH(url) {
      if (api.openUrl) api.openUrl(url);
      else window.open(url, '_blank');
    }
    if (githubLink) githubLink.addEventListener('click', function (e) { e.preventDefault(); openGH('https://github.com/GeLith'); });
    if (feedback) feedback.addEventListener('click', function (e) { e.preventDefault(); openGH('https://github.com/GeLith/issues'); });
    if (update) update.addEventListener('click', function (e) { e.preventDefault(); openGH('https://github.com/GeLith/releases'); });

    document.addEventListener('viewchange', function (e) {
      if (e && e.detail && e.detail.view === 'settings') refreshSettings();
    });
  })();

})();
