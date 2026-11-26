(function () {
  'use strict';

  const host = ((window.location && window.location.hostname) || '').toLowerCase();
  if (host !== 'tiktok.com' && !host.endsWith('.tiktok.com')) {
    return;
  }

  if (window.__ztikInjected) {
    return;
  }
  window.__ztikInjected = true;

  const STYLE_ID = 'ztik-style';
  const SPLASH_STYLE_ID = 'ztik-splash-style';
  const SPLASH_OVERLAY_ID = 'ztik-splash-overlay';
  const SPLASH_STORAGE_KEY = 'ztik_intro_done';
  const SPLASH_MAX_WAIT_MS = 1800;
  const SPLASH_EXIT_MS = 350;
  const BLOB_REVOKE_DELAY_MS = 3500;
  const HIDDEN_ATTR = 'data-ztik-hidden';
  const OFFSCREEN_CARD_ATTR = 'data-ztik-offscreen-card';
  const DEBOUNCE_MS = 120;
  const MEDIA_CACHE_LIMIT = 18;
  const MEDIA_MAX_VIDEO_URLS = 4;
  const MEDIA_MAX_IMAGE_MIRRORS = 3;
  const VIDEO_VIEWPORT_MARGIN = '100% 0px 100% 0px';
  const CARD_VIEWPORT_MARGIN = '80% 0px 80% 0px';
  const TITLEBAR_ID = 'ztik-titlebar';
  const TOAST_ID = 'ztik-download-toast';
  const mediaCache = new Map();

  const TIKTOK_NOTE_PATH =
    'M23 7h6c.4 4.8 4.1 8.2 9.5 8.6v5.8c-3.8-.2-7.1-1.5-9.5-3.4V32a11 11 0 1 1-9-10.82v6.24A5 5 0 1 0 23 32V7Z';

  const SPLASH_CSS =
    '#' + SPLASH_OVERLAY_ID + '{' +
    'position:fixed;inset:0;z-index:2147483647;display:flex;flex-direction:column;' +
    'align-items:center;justify-content:center;gap:22px;background:#000000;' +
    'opacity:1;transform:scale(1);' +
    'transition:opacity ' + SPLASH_EXIT_MS + 'ms ease-out,transform ' + SPLASH_EXIT_MS + 'ms ease-out;' +
    '}' +
    '#' + SPLASH_OVERLAY_ID + '.ztik-splash-exit{' +
    'opacity:0;transform:scale(1.04);pointer-events:none;' +
    '}' +
    '@media (prefers-reduced-motion: reduce){' +
    '#' + SPLASH_OVERLAY_ID + '{transition:opacity 150ms ease-out;}' +
    '#' + SPLASH_OVERLAY_ID + '.ztik-splash-exit{transform:none;}' +
    '}' +
    '#' + SPLASH_OVERLAY_ID + ' .ztik-splash-logo{width:84px;height:84px;}' +
    '#' + SPLASH_OVERLAY_ID + ' .ztik-splash-logo svg{display:block;width:100%;height:100%;overflow:visible;}' +
    '#' + SPLASH_OVERLAY_ID + ' .ztik-splash-bar{' +
    'position:relative;width:112px;height:3px;border-radius:999px;' +
    'background:rgba(255,255,255,0.14);overflow:clip;' +
    '}' +
    '#' + SPLASH_OVERLAY_ID + ' .ztik-splash-bar-fill{' +
    'position:absolute;inset:0;width:52%;border-radius:999px;' +
    'background:linear-gradient(90deg,#25f4ee 0%,#ffffff 50%,#fe2c55 100%);' +
    'transform:translateX(46%);' +
    '}' +
    '@media (prefers-reduced-motion: no-preference){' +
    '#' + SPLASH_OVERLAY_ID + ' .ztik-splash-bar-fill{' +
    'animation:ztik-handoff-sweep 900ms cubic-bezier(0.4,0,0.2,1) infinite;' +
    '}' +
    '}' +
    '@keyframes ztik-handoff-sweep{' +
    '0%{transform:translateX(-105%);}' +
    '100%{transform:translateX(205%);}' +
    '}';

  let splashDismissed = false;
  let splashExitTimer = null;
  let splashMaxTimer = null;
  let splashReadyObserver = null;

  function isIntroDone() {
    try {
      return window.sessionStorage && window.sessionStorage.getItem(SPLASH_STORAGE_KEY) === '1';
    } catch (_) {
      return false;
    }
  }

  function markIntroDone() {
    try {
      if (window.sessionStorage) {
        window.sessionStorage.setItem(SPLASH_STORAGE_KEY, '1');
      }
    } catch (_) {}
  }

  function createSplashSvg() {
    return (
      '<svg viewBox="0 0 48 48" aria-hidden="true" focusable="false">' +
      '<path d="' + TIKTOK_NOTE_PATH + '" fill="#25F4EE" transform="translate(-2.2 -2.2)" opacity="0.95" />' +
      '<path d="' + TIKTOK_NOTE_PATH + '" fill="#FE2C55" transform="translate(2.2 2.2)" opacity="0.95" />' +
      '<path d="' + TIKTOK_NOTE_PATH + '" fill="#FFFFFF" />' +
      '</svg>'
    );
  }

  function mountSplashOverlay() {
    if (splashDismissed) {
      return true;
    }
    const root = document.documentElement;
    if (!root) {
      return false;
    }

    if (!document.getElementById(SPLASH_STYLE_ID)) {
      const styleEl = document.createElement('style');
      styleEl.id = SPLASH_STYLE_ID;
      styleEl.textContent = SPLASH_CSS;
      (document.head || root).appendChild(styleEl);
    }

    if (!document.getElementById(SPLASH_OVERLAY_ID)) {
      const overlay = document.createElement('div');
      overlay.id = SPLASH_OVERLAY_ID;
      overlay.setAttribute('role', 'presentation');
      overlay.setAttribute('aria-hidden', 'true');
      overlay.innerHTML =
        '<div class="ztik-splash-logo">' + createSplashSvg() + '</div>' +
        '<div class="ztik-splash-bar"><div class="ztik-splash-bar-fill"></div></div>';
      root.appendChild(overlay);
    }
    return true;
  }

  function removeSplashElements() {
    if (splashReadyObserver) {
      splashReadyObserver.disconnect();
      splashReadyObserver = null;
    }
    if (splashExitTimer !== null) {
      clearTimeout(splashExitTimer);
      splashExitTimer = null;
    }
    if (splashMaxTimer !== null) {
      clearTimeout(splashMaxTimer);
      splashMaxTimer = null;
    }
    const overlay = document.getElementById(SPLASH_OVERLAY_ID);
    if (overlay) {
      overlay.remove();
    }
    const styleEl = document.getElementById(SPLASH_STYLE_ID);
    if (styleEl) {
      styleEl.remove();
    }
  }

  function dismissSplashOverlay() {
    if (splashDismissed) {
      return;
    }
    splashDismissed = true;
    markIntroDone();

    if (splashReadyObserver) {
      splashReadyObserver.disconnect();
      splashReadyObserver = null;
    }
    if (splashMaxTimer !== null) {
      clearTimeout(splashMaxTimer);
      splashMaxTimer = null;
    }

    const overlay = document.getElementById(SPLASH_OVERLAY_ID);
    if (!overlay) {
      removeSplashElements();
      return;
    }

    overlay.addEventListener(
      'transitionend',
      (event) => {
        if (event.target === overlay && event.propertyName === 'opacity') {
          removeSplashElements();
        }
      },
      { once: true }
    );

    requestAnimationFrame(() => {
      overlay.classList.add('ztik-splash-exit');
      if (typeof overlay.getAnimations === 'function' && overlay.getAnimations().length === 0) {
        removeSplashElements();
      }
    });

    splashExitTimer = window.setTimeout(removeSplashElements, SPLASH_EXIT_MS + 120);
  }

  function isTikTokContentReady() {
    if (document.readyState === 'complete') {
      return true;
    }
    if (document.readyState === 'loading' || !document.body) {
      return false;
    }
    return Boolean(
      document.querySelector(
        'video, main, [data-e2e="recommend-list-container"], [data-e2e="nav-foryou"], #main-content-homepage_hot'
      )
    );
  }

  function installSplashHandoff() {
    if (isIntroDone()) {
      splashDismissed = true;
      return;
    }

    if (!mountSplashOverlay() && typeof MutationObserver !== 'undefined') {
      const rootObserver = new MutationObserver(() => {
        if (mountSplashOverlay()) {
          rootObserver.disconnect();
        }
      });
      rootObserver.observe(document, { childList: true });
    }

    splashMaxTimer = window.setTimeout(dismissSplashOverlay, SPLASH_MAX_WAIT_MS);

    function checkReadyAndDismiss() {
      if (splashDismissed) {
        return;
      }
      if (isTikTokContentReady()) {
        if (splashReadyObserver) {
          splashReadyObserver.disconnect();
          splashReadyObserver = null;
        }
        window.setTimeout(dismissSplashOverlay, 120);
      }
    }

    window.addEventListener('load', () => {
      if (!splashDismissed) {
        window.setTimeout(dismissSplashOverlay, 80);
      }
    }, { once: true });

    const watchDomReady = () => {
      mountSplashOverlay();
      checkReadyAndDismiss();
      if (!splashDismissed && document.body && typeof MutationObserver !== 'undefined') {
        splashReadyObserver = new MutationObserver(checkReadyAndDismiss);
        splashReadyObserver.observe(document.body, { childList: true, subtree: true });
      }
    };

    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', watchDomReady, { once: true });
    } else {
      watchDomReady();
    }
  }

  installSplashHandoff();

  const BLOCKED_E2E_VALUES = new Set([
    'upload-icon',
    'nav-upload',
    'upload-btn',
    'header-upload',
    'get-coins',
    'get-coin',
    'nav-coins',
    'profile-get-coins',
    'coin-entry',
    'get-app',
    'top-get-app',
    'nav-get-app',
    'get-app-btn',
    'bottom-get-app',
    'download-app',
    'install-app',
    'floating-get-app',
    'promote-download',
    'promote',
    'promote-post',
    'promote-btn',
    'video-promote',
    'profile-promote',
  ]);

  const BLOCKED_TEXT_LABELS = new Set([
    'get coins',
    'get app',
    'get tiktok app',
    'promote',
    'promote post',
    'nhận xu',
    'mua xu',
    'tải ứng dụng',
    'quảng bá',
    'quảng bá bài đăng',
  ]);

  const NAV_ONLY_TEXT_LABELS = new Set(['upload', 'tải lên']);

  const ztikPausedVideos = new WeakSet();
  const observedVideos = new WeakSet();
  const offscreenVideos = new WeakSet();
  const savedVideoPreload = new WeakMap();
  const observedCards = new WeakSet();
  const scannedHydrationObjects = new WeakSet();
  const lastScriptFingerprints = Object.create(null);
  let videoIntersectionObserver = null;
  let cardIntersectionObserver = null;
  let hideTimer = null;
  let maintenanceFrame = 0;
  let toastTimer = null;
  let activeVideoLayerRef = null;
  let downloadInFlight = false;

  function trustedMediaUrl(rawUrl) {
    if (typeof rawUrl !== 'string') {
      return '';
    }
    const trimmed = rawUrl.trim();
    if (!trimmed) {
      return '';
    }
    try {
      const url = new URL(trimmed, window.location.href);
      const urlHost = url.hostname.toLowerCase();
      const trusted = [
        'tiktok.com',
        'tiktokv.com',
        'bytedance.com',
        'tiktokcdn.com',
        'tiktokcdn-us.com',
        'tiktokcdn-eu.com',
        'tiktokcdn-in.com',
        'ibyteimg.com',
        'ibytedtos.com',
        'byteoversea.com',
      ];
      if (
        url.protocol !== 'https:' ||
        url.username ||
        url.password ||
        !trusted.some((domain) => urlHost === domain || urlHost.endsWith('.' + domain))
      ) {
        return '';
      }
      // Keep signed CDN query parameters 100% verbatim without normalizing/stripping.
      return /^https:\/\//i.test(trimmed) ? trimmed : url.href;
    } catch (_) {
      return '';
    }
  }

  function urlList(value) {
    if (typeof value === 'string') {
      return [value];
    }
    if (!value || typeof value !== 'object') {
      return [];
    }
    const list = value.urlList || value.url_list || value.UrlList || value.urls;
    if (Array.isArray(list)) {
      return list.filter((item) => typeof item === 'string');
    }
    const url = value.url || value.Url;
    return typeof url === 'string' ? [url] : [];
  }

  function pushUniqueTrustedUrl(target, rawUrl) {
    const safe = trustedMediaUrl(rawUrl);
    if (safe && !target.includes(safe)) {
      target.push(safe);
    }
  }

  function collectOrderedImageMirrors(urls) {
    const preferred = [];
    const heicFallback = [];
    for (let i = 0; i < urls.length; i += 1) {
      const safe = trustedMediaUrl(urls[i]);
      if (!safe) {
        continue;
      }
      let isHeic = false;
      try {
        isHeic = /\.(heic|heif)$/i.test(new URL(safe).pathname.toLowerCase());
      } catch (_) {}
      const bucket = isHeic ? heicFallback : preferred;
      if (!bucket.includes(safe)) {
        bucket.push(safe);
      }
    }
    return preferred.concat(heicFallback);
  }

  function cachePost(candidate) {
    if (!candidate || typeof candidate !== 'object') {
      return null;
    }
    const video = candidate.video && typeof candidate.video === 'object' ? candidate.video : null;
    const imagePost =
      (candidate.imagePost && typeof candidate.imagePost === 'object' && candidate.imagePost) ||
      (candidate.image_post && typeof candidate.image_post === 'object' && candidate.image_post) ||
      null;
    if (!video && !imagePost) {
      return null;
    }

    const id = String(candidate.id || candidate.aweme_id || candidate.itemId || candidate.item_id || '');
    if (!id) {
      return null;
    }
    const authorData = candidate.author || candidate.authorInfo || candidate.author_info || {};
    const author = String(
      authorData.uniqueId || authorData.unique_id || authorData.nickname || candidate.authorName || ''
    );
    const videoUrls = [];
    if (video) {
      const bitrates = Array.isArray(video.bitrateInfo)
        ? video.bitrateInfo.slice()
        : Array.isArray(video.bitrate_info) ? video.bitrate_info.slice() : [];
      bitrates.sort((left, right) => Number(right.Bitrate || right.bitrate || 0) - Number(left.Bitrate || left.bitrate || 0));
      for (let i = 0; i < bitrates.length && videoUrls.length < MEDIA_MAX_VIDEO_URLS; i += 1) {
        const playAddr = bitrates[i].PlayAddr || bitrates[i].playAddr || bitrates[i].play_addr;
        const urls = urlList(playAddr);
        for (let j = 0; j < urls.length && videoUrls.length < MEDIA_MAX_VIDEO_URLS; j += 1) {
          pushUniqueTrustedUrl(videoUrls, urls[j]);
        }
      }
      const playUrls = urlList(video.playAddr || video.play_addr);
      for (let i = 0; i < playUrls.length && videoUrls.length < MEDIA_MAX_VIDEO_URLS; i += 1) {
        pushUniqueTrustedUrl(videoUrls, playUrls[i]);
      }
    }

    const imageSlides = [];
    const seenPrimaryImages = new Set();
    if (imagePost && Array.isArray(imagePost.images)) {
      for (let i = 0; i < imagePost.images.length; i += 1) {
        const image = imagePost.images[i] || {};
        const urls = urlList(image.imageURL || image.image_url || image.displayImage || image.display_image);
        const mirrors = collectOrderedImageMirrors(urls).slice(0, MEDIA_MAX_IMAGE_MIRRORS);
        if (mirrors.length > 0 && !seenPrimaryImages.has(mirrors[0])) {
          seenPrimaryImages.add(mirrors[0]);
          imageSlides.push(mirrors);
        }
      }
    }
    if (videoUrls.length === 0 && imageSlides.length === 0) {
      return null;
    }

    // Store strictly minimal primitive string data (never raw JSON, DOM nodes, or React objects).
    const entry = {
      id,
      author,
      videoUrls,
      imageSlides,
    };
    mediaCache.delete(id);
    mediaCache.set(id, entry);
    while (mediaCache.size > MEDIA_CACHE_LIMIT) {
      mediaCache.delete(mediaCache.keys().next().value);
    }
    return entry;
  }

  function getCachedMedia(id) {
    if (!id || !mediaCache.has(id)) {
      return null;
    }
    const entry = mediaCache.get(id);
    mediaCache.delete(id);
    mediaCache.set(id, entry);
    return entry;
  }

  function isSkippableHostObject(value) {
    if (!value || typeof value !== 'object') {
      return true;
    }
    if (value === window || value === document) {
      return true;
    }
    if (typeof Node !== 'undefined' && value instanceof Node) {
      return true;
    }
    return false;
  }

  function scanMediaPayload(payload, options) {
    const maxDepth = (options && options.maxDepth) || 14;
    const maxVisits = (options && options.maxVisits) || 5000;
    const seen = (options && options.seen) || new WeakSet();
    const collected = (options && options.collected) || null;
    let visited = 0;

    function visit(value, depth) {
      if (
        depth > maxDepth ||
        visited >= maxVisits ||
        isSkippableHostObject(value) ||
        seen.has(value)
      ) {
        return;
      }
      seen.add(value);
      visited += 1;
      if (Array.isArray(value)) {
        for (let i = 0; i < value.length && i < 300; i += 1) {
          visit(value[i], depth + 1);
        }
        return;
      }
      const cached = cachePost(value);
      if (cached && collected) {
        collected.push(cached);
      }
      let keys;
      try {
        keys = Object.keys(value);
      } catch (_) {
        return;
      }
      for (let i = 0; i < keys.length && i < 160; i += 1) {
        const key = keys[i];
        if (key === 'return' || key === 'child' || key === 'sibling' || key === 'stateNode' || key === '_owner') {
          continue;
        }
        let child;
        try {
          child = value[key];
        } catch (_) {
          continue;
        }
        visit(child, depth + 1);
      }
    }
    visit(payload, 0);
  }

  function fingerprintJsonText(raw) {
    const len = raw.length;
    return len + ':' + raw.slice(0, 64) + ':' + raw.slice(Math.max(0, len - 64));
  }

  function scanJsonScriptById(scriptId, cacheKey) {
    try {
      const script = document.getElementById(scriptId);
      let raw = script && typeof script.textContent === 'string' ? script.textContent.trim() : '';
      if (!raw) {
        return;
      }
      const fingerprint = fingerprintJsonText(raw);
      if (fingerprint === lastScriptFingerprints[cacheKey]) {
        raw = '';
        return;
      }
      lastScriptFingerprints[cacheKey] = fingerprint;
      let parsed = JSON.parse(raw);
      raw = '';
      scanMediaPayload(parsed);
      parsed = null;
    } catch (_) {}
  }

  function scanHydrationData() {
    try {
      const data = window.__UNIVERSAL_DATA_FOR_REHYDRATION__;
      if (data && typeof data === 'object' && !scannedHydrationObjects.has(data)) {
        scannedHydrationObjects.add(data);
        scanMediaPayload(data);
      }
    } catch (_) {}
    scanJsonScriptById('__UNIVERSAL_DATA_FOR_REHYDRATION__', 'lastUniversalScript');

    try {
      const sigi = window.SIGI_STATE;
      if (sigi && typeof sigi === 'object' && !scannedHydrationObjects.has(sigi)) {
        scannedHydrationObjects.add(sigi);
        scanMediaPayload(sigi);
      }
    } catch (_) {}
    scanJsonScriptById('SIGI_STATE', 'lastSigiScript');
  }

  function extractMediaFromReactNodes(nodes, expectedId) {
    const visitedNodes = new Set();
    const seenObjects = new WeakSet();
    const collected = [];

    function scanCandidate(value) {
      if (!value || typeof value !== 'object') {
        return;
      }
      scanMediaPayload(value, {
        maxDepth: 8,
        maxVisits: 1200,
        seen: seenObjects,
        collected,
      });
    }

    for (let i = 0; i < nodes.length; i += 1) {
      const node = nodes[i];
      if (!node || typeof node !== 'object' || visitedNodes.has(node)) {
        continue;
      }
      visitedNodes.add(node);

      let keys;
      try {
        keys = Object.keys(node);
      } catch (_) {
        continue;
      }

      for (let j = 0; j < keys.length; j += 1) {
        const key = keys[j];
        if (key.startsWith('__reactProps$')) {
          try {
            scanCandidate(node[key]);
          } catch (_) {}
        } else if (key.startsWith('__reactFiber$') || key.startsWith('__reactInternalInstance$')) {
          let fiber;
          try {
            fiber = node[key];
          } catch (_) {
            fiber = null;
          }
          let hops = 0;
          while (fiber && typeof fiber === 'object' && hops < 24) {
            scanCandidate(fiber.memoizedProps);
            scanCandidate(fiber.pendingProps);
            scanCandidate(fiber.memoizedState);
            if (expectedId && collected.some((entry) => entry.id === expectedId)) {
              break;
            }
            fiber = fiber.return;
            hops += 1;
          }
        }
      }

      if (expectedId) {
        const exact = collected.find((entry) => entry.id === expectedId);
        if (exact) {
          return exact;
        }
      }
    }

    if (expectedId) {
      const exact = collected.find((entry) => entry.id === expectedId);
      if (exact) {
        return exact;
      }
    }
    return collected.length > 0 ? collected[0] : null;
  }

  function ensureStyle() {
    const css = typeof window.__ztikCss === 'string' ? window.__ztikCss : '';
    if (!css) {
      return;
    }

    let styleEl = document.getElementById(STYLE_ID);
    if (!styleEl) {
      styleEl = document.createElement('style');
      styleEl.id = STYLE_ID;
      styleEl.textContent = css;
    } else if (styleEl.textContent !== css) {
      styleEl.textContent = css;
    }

    const head = document.head;
    const mount = head || document.documentElement;
    if (mount && (styleEl.parentNode !== mount || (head && head.lastElementChild !== styleEl))) {
      mount.appendChild(styleEl);
    }
  }

  function containsMiniPlayerContent(el) {
    if (!el || el.nodeType !== 1) {
      return true;
    }
    const protectedSelector =
      'main, #main-content-homepage_hot, video, [data-e2e="recommend-list-item-container"]';
    return el.matches(protectedSelector) || Boolean(el.querySelector(protectedSelector));
  }

  function findMiniPlayerSidebar() {
    const selectors = [
      '[data-e2e="nav-foryou"]',
      '[data-e2e="nav-explore"]',
      '[data-e2e="tiktok-logo"]',
    ];
    for (let i = 0; i < selectors.length; i += 1) {
      const node = document.querySelector(selectors[i]);
      if (node) {
        return node;
      }
    }
    return null;
  }

  function refreshMiniPlayerMode() {
    const marked = document.querySelectorAll('[data-ztik-mini-hide="1"]');
    const html = document.documentElement;
    const miniMode = window.__ZTIK_MINI === true || Boolean(html && html.classList.contains('ztik-mini-ui'));
    if (!miniMode) {
      for (let i = 0; i < marked.length; i += 1) {
        marked[i].removeAttribute('data-ztik-mini-hide');
      }
      return;
    }

    if (html && window.__ZTIK_MINI === true && !html.classList.contains('ztik-mini')) {
      html.classList.add('ztik-mini');
    }

    const navNode = findMiniPlayerSidebar();
    if (!navNode) {
      for (let i = 0; i < marked.length; i += 1) {
        marked[i].removeAttribute('data-ztik-mini-hide');
      }
      return;
    }

    let sidebar = navNode;
    let parent = sidebar.parentElement;
    while (
      parent &&
      parent !== document.body &&
      parent !== document.documentElement &&
      !containsMiniPlayerContent(parent)
    ) {
      sidebar = parent;
      parent = parent.parentElement;
    }

    for (let i = 0; i < marked.length; i += 1) {
      if (marked[i] !== sidebar) {
        marked[i].removeAttribute('data-ztik-mini-hide');
      }
    }
    if (sidebar.getAttribute('data-ztik-mini-hide') !== '1') {
      sidebar.setAttribute('data-ztik-mini-hide', '1');
    }
  }

  function containsVideo(el) {
    if (!el || el.nodeType !== 1) {
      return true;
    }
    if (el === document.documentElement || el === document.body || el.tagName === 'VIDEO') {
      return true;
    }
    if (el.tagName === 'MAIN' || el.querySelector('video')) {
      return true;
    }
    return false;
  }

  function hideElementSafely(el) {
    if (!el || containsVideo(el)) {
      return;
    }
    if (el.getAttribute(HIDDEN_ATTR) !== 'true') {
      el.setAttribute(HIDDEN_ATTR, 'true');
    }
    if (el.style && el.style.display !== 'none') {
      el.style.setProperty('display', 'none', 'important');
    }

    const parentItem = el.closest('li, [role="menuitem"]');
    if (
      parentItem &&
      parentItem !== el &&
      !containsVideo(parentItem) &&
      parentItem.closest('nav, aside, header, [role="navigation"], [role="menu"]')
    ) {
      const links = parentItem.querySelectorAll('a[href], button');
      if (links.length <= 1) {
        if (parentItem.getAttribute(HIDDEN_ATTR) !== 'true') {
          parentItem.setAttribute(HIDDEN_ATTR, 'true');
        }
        if (parentItem.style && parentItem.style.display !== 'none') {
          parentItem.style.setProperty('display', 'none', 'important');
        }
      }
    }
  }

  function isBlockedHref(rawHref) {
    if (!rawHref) {
      return false;
    }
    try {
      const parsed = new URL(rawHref, window.location.origin);
      const linkHost = parsed.hostname.toLowerCase();
      if (
        linkHost === 'studio.tiktok.com' ||
        linkHost.endsWith('.studio.tiktok.com') ||
        linkHost.includes('tiktokstudio') ||
        linkHost === 'ads.tiktok.com' ||
        linkHost.endsWith('.onelink.me') ||
        linkHost === 'onelink.me'
      ) {
        return true;
      }

      if (linkHost === 'tiktok.com' || linkHost.endsWith('.tiktok.com')) {
        const path = parsed.pathname.toLowerCase();
        const trimmed = path.replace(/^\/+/, '');
        if (
          trimmed.startsWith('upload') ||
          trimmed.startsWith('tiktokstudio') ||
          trimmed.startsWith('creator-center/upload') ||
          trimmed.startsWith('coin') ||
          trimmed.startsWith('wallet') ||
          trimmed.startsWith('download') ||
          trimmed.startsWith('promote')
        ) {
          return true;
        }
        if (path.split('/').includes('tiktokstudio')) {
          return true;
        }
      }
    } catch (_) {
      return false;
    }
    return false;
  }

  const TITLEBAR_HEIGHT_PX = 34;
  const VIEWPORT_LAYOUT_ATTRS = [
    'data-ztik-fixed-top',
    'data-ztik-fixed-fullheight',
    'data-ztik-sticky-top',
    'data-ztik-fullheight',
    'data-ztik-maxheight',
  ];

  const COIN_PROMO_DISMISS_LABELS = new Set([
    'not now',
    'để sau',
    'maybe later',
    'no thanks',
    'không phải bây giờ',
    'bỏ qua',
    'skip',
    'later',
  ]);

  const COIN_PROMO_CTA_LABELS = new Set([
    'recharge',
    'get coins',
    'nạp xu',
    'nhận xu',
    'nạp ngay',
    'mua xu',
  ]);

  const PROTECTED_MODAL_PATTERN =
    /\b(log\s?in|sign\s?in|sign\s?up|captcha|verify|verification|puzzle|slider|otp|password|passcode|edit\s+profile|comments?)\b|đăng\s*nhập|đăng\s*ký|xác\s*minh|mã\s*xác|mật\s*khẩu|chỉnh\s*sửa\s*hồ\s*sơ|bình\s*luận/i;

  function isZtikOwnElement(el) {
    if (!el || el.nodeType !== 1) {
      return true;
    }
    return Boolean(
      el.id === TITLEBAR_ID ||
      el.id === TOAST_ID ||
      el.id === SPLASH_OVERLAY_ID ||
      el.closest('#' + TITLEBAR_ID + ', #' + TOAST_ID + ', #' + SPLASH_OVERLAY_ID)
    );
  }

  function hasScrollableAncestor(el) {
    let parent = el ? el.parentElement : null;
    while (parent && parent !== document.body && parent !== document.documentElement) {
      const style = window.getComputedStyle(parent);
      if (style && /(auto|scroll)/.test(style.overflowY || '')) {
        return true;
      }
      parent = parent.parentElement;
    }
    return false;
  }

  function clearViewportLayoutMarkers() {
    for (let i = 0; i < VIEWPORT_LAYOUT_ATTRS.length; i += 1) {
      const attr = VIEWPORT_LAYOUT_ATTRS[i];
      const nodes = document.querySelectorAll('[' + attr + '="1"]');
      for (let j = 0; j < nodes.length; j += 1) {
        nodes[j].removeAttribute(attr);
      }
    }
  }

  function syncViewportLayout() {
    const html = document.documentElement;
    if (!html) {
      return;
    }

    const titlebarHeight = isNativeDecorationsEnabled() ? 0 : TITLEBAR_HEIGHT_PX;
    const heightCss = titlebarHeight + 'px';
    if (html.style.getPropertyValue('--ztik-titlebar-h') !== heightCss) {
      html.style.setProperty('--ztik-titlebar-h', heightCss);
    }

    if (titlebarHeight === 0) {
      clearViewportLayoutMarkers();
      return;
    }

    const viewportHeight = window.innerHeight || 0;
    if (viewportHeight <= titlebarHeight + 80) {
      return;
    }
    const availableHeight = viewportHeight - titlebarHeight;

    const candidates = new Set();
    const addCandidate = (el) => {
      if (
        !el ||
        el.nodeType !== 1 ||
        el === html ||
        el === document.body ||
        isZtikOwnElement(el)
      ) {
        return;
      }
      candidates.add(el);
    };

    const addAncestors = (el, maxHops) => {
      let current = el;
      let hops = 0;
      while (current && current !== document.body && current !== html && hops < maxHops) {
        addCandidate(current);
        current = current.parentElement;
        hops += 1;
      }
    };

    const navAnchors = document.querySelectorAll(
      '[data-e2e="tiktok-logo"], [data-e2e="nav-foryou"], [data-e2e="nav-explore"]'
    );
    for (let i = 0; i < navAnchors.length; i += 1) {
      addAncestors(navAnchors[i], 10);
    }

    const roots = [document.body, document.getElementById('app'), document.getElementById('__next')];
    for (let i = 0; i < roots.length; i += 1) {
      const rootEl = roots[i];
      if (!rootEl) {
        continue;
      }
      addCandidate(rootEl);
      const children = rootEl.children;
      for (let j = 0; j < children.length && j < 40; j += 1) {
        const child = children[j];
        addCandidate(child);
        const grandChildren = child.children;
        for (let k = 0; k < grandChildren.length && k < 20; k += 1) {
          addCandidate(grandChildren[k]);
        }
      }
    }

    const structuralNodes = document.querySelectorAll(
      'header, nav, aside, main, article, [role="banner"], [role="navigation"], [role="dialog"], ' +
      '#main-content-homepage_hot, [data-e2e="recommend-list-container"], ' +
      '[data-e2e="recommend-list-item-container"], [data-e2e="browse-video"], ' +
      '[data-e2e="detail-video"], [data-e2e="video-player-container"]'
    );
    for (let i = 0; i < structuralNodes.length; i += 1) {
      addCandidate(structuralNodes[i]);
    }

    const activeVideo = findActiveVideo();
    if (activeVideo) {
      addAncestors(activeVideo.parentElement, 12);
    }

    const playerShellSelector =
      '#app, #__next, main, #main-content-homepage_hot, article, ' +
      '[data-e2e="recommend-list-container"], [data-e2e="recommend-list-item-container"], ' +
      '[data-e2e="video-player-container"], [data-e2e="browse-video"], [data-e2e="detail-video"]';

    candidates.forEach((el) => {
      if (!el.isConnected || el.tagName === 'VIDEO' || el.getAttribute(HIDDEN_ATTR) === 'true') {
        return;
      }
      const style = window.getComputedStyle(el);
      if (!style || style.display === 'none') {
        return;
      }

      const rect = el.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) {
        return;
      }

      if (style.position === 'fixed') {
        const alreadyFixed =
          el.getAttribute('data-ztik-fixed-top') === '1' ||
          el.getAttribute('data-ztik-fixed-fullheight') === '1';
        const topVal = parseFloat(style.top);
        const touchesTopBar =
          alreadyFixed ||
          (rect.height >= 24 &&
            rect.top >= -4 &&
            rect.top < titlebarHeight - 1 &&
            (Number.isNaN(topVal) || topVal < titlebarHeight - 1));

        if (touchesTopBar) {
          const spansFullHeight =
            style.bottom === '0px' ||
            Math.abs(rect.height - viewportHeight) <= 10 ||
            Math.abs(rect.height - availableHeight) <= 10 ||
            rect.bottom >= viewportHeight - 4;
          if (spansFullHeight) {
            el.removeAttribute('data-ztik-fixed-top');
            if (el.getAttribute('data-ztik-fixed-fullheight') !== '1') {
              el.setAttribute('data-ztik-fixed-fullheight', '1');
            }
          } else {
            el.removeAttribute('data-ztik-fixed-fullheight');
            if (el.getAttribute('data-ztik-fixed-top') !== '1') {
              el.setAttribute('data-ztik-fixed-top', '1');
            }
          }
        }
        return;
      }

      if (style.position === 'sticky') {
        const topVal = parseFloat(style.top);
        if (
          !Number.isNaN(topVal) &&
          topVal >= -2 &&
          topVal < titlebarHeight - 1 &&
          !hasScrollableAncestor(el)
        ) {
          if (el.getAttribute('data-ztik-sticky-top') !== '1') {
            el.setAttribute('data-ztik-sticky-top', '1');
          }
        }
        return;
      }

      const isPlayerOrFeedShell = el.matches(playerShellSelector);
      const isScrollOrClipped = /(auto|scroll|hidden|clip)/.test(style.overflowY || '');

      if (
        Math.abs(rect.height - viewportHeight) <= 8 &&
        (isScrollOrClipped || isPlayerOrFeedShell || el.scrollHeight <= viewportHeight + 24)
      ) {
        if (el.getAttribute('data-ztik-fullheight') !== '1') {
          el.setAttribute('data-ztik-fullheight', '1');
        }
        return;
      }

      if (
        (isPlayerOrFeedShell || (activeVideo && el.contains(activeVideo))) &&
        rect.height > availableHeight + 2 &&
        rect.height <= viewportHeight + 60 &&
        (isScrollOrClipped || rect.bottom > viewportHeight - 2)
      ) {
        if (el.getAttribute('data-ztik-maxheight') !== '1') {
          el.setAttribute('data-ztik-maxheight', '1');
        }
      }
    });
  }

  function isProtectedModal(modal, text) {
    if (!modal || modal.nodeType !== 1) {
      return true;
    }
    if (
      modal.querySelector(
        'input, textarea, form, iframe, video, [contenteditable="true"], [contenteditable=""], [role="textbox"]'
      )
    ) {
      return true;
    }
    const meta = [
      modal.id || '',
      modal.getAttribute('data-e2e') || '',
      modal.getAttribute('aria-label') || '',
      text || '',
    ].join(' ');
    return PROTECTED_MODAL_PATTERN.test(meta);
  }

  function isCoinPromoText(text) {
    if (!text || text.length > 900) {
      return false;
    }
    if (
      text.includes('recharge and save') ||
      text.includes('get coins') ||
      text.includes('nạp xu') ||
      text.includes('nhận xu') ||
      text.includes('mua xu') ||
      text.includes('custom recharge')
    ) {
      return true;
    }
    return text.includes('save up to') && (text.includes('coin') || text.includes('xu') || text.includes('recharge'));
  }

  function dismissCoinPromoModals() {
    const rawModals = document.querySelectorAll(
      '[role="dialog"], [aria-modal="true"], [data-e2e*="modal" i], [data-e2e*="dialog" i]'
    );
    const seen = new Set();
    for (let i = 0; i < rawModals.length; i += 1) {
      const candidate = rawModals[i];
      const modal = candidate.closest('[role="dialog"], [aria-modal="true"]') || candidate;
      if (
        !modal ||
        seen.has(modal) ||
        isZtikOwnElement(modal) ||
        modal.dataset.ztikDismissed === '1' ||
        modal.getAttribute(HIDDEN_ATTR) === 'true' ||
        !isElementVisible(modal)
      ) {
        continue;
      }
      seen.add(modal);

      const text = (modal.textContent || '').replace(/\s+/g, ' ').trim().toLowerCase();
      if (!isCoinPromoText(text) || isProtectedModal(modal, text)) {
        continue;
      }

      const controls = modal.querySelectorAll('button, [role="button"], a[href]');
      let dismissBtn = null;
      let closeBtn = null;
      let hasRechargeCta = false;

      for (let j = 0; j < controls.length; j += 1) {
        const control = controls[j];
        if (!isElementVisible(control)) {
          continue;
        }
        const label = (control.textContent || '').replace(/\s+/g, ' ').trim().toLowerCase();
        const aria = (control.getAttribute('aria-label') || '').trim().toLowerCase();
        const e2e = (control.getAttribute('data-e2e') || '').trim().toLowerCase();
        const href = (control.getAttribute('href') || '').trim().toLowerCase();

        if (!dismissBtn && COIN_PROMO_DISMISS_LABELS.has(label)) {
          dismissBtn = control;
        }
        if (
          !closeBtn &&
          (/^(close|đóng|dismiss)/.test(aria) || /modal-close|close-btn|close-icon/.test(e2e))
        ) {
          closeBtn = control;
        }
        if (COIN_PROMO_CTA_LABELS.has(label) || href.includes('/coin')) {
          hasRechargeCta = true;
        }
      }

      if (!dismissBtn && !hasRechargeCta) {
        continue;
      }

      modal.dataset.ztikDismissed = '1';
      if (dismissBtn) {
        dismissBtn.click();
      } else if (closeBtn) {
        closeBtn.click();
      } else {
        hideElementSafely(modal);
      }
    }
  }

  function hideUnwantedElements() {
    ensureStyle();
    ensureTitlebar();
    syncViewportLayout();
    scanHydrationData();
    refreshMiniPlayerMode();
    dismissCoinPromoModals();
    observeFeedMedia();

    const root = document.body || document.documentElement;
    if (!root) {
      return;
    }

    const e2eNodes = root.querySelectorAll('[data-e2e]');
    for (let i = 0; i < e2eNodes.length; i += 1) {
      const node = e2eNodes[i];
      const value = (node.getAttribute('data-e2e') || '').trim().toLowerCase();
      if (BLOCKED_E2E_VALUES.has(value)) {
        hideElementSafely(node);
      }
    }

    const anchors = root.querySelectorAll('a[href]');
    for (let i = 0; i < anchors.length; i += 1) {
      const anchor = anchors[i];
      if (isBlockedHref(anchor.getAttribute('href'))) {
        hideElementSafely(anchor);
      }
    }

    const interactives = root.querySelectorAll('button, [role="menuitem"], [role="button"]');
    for (let i = 0; i < interactives.length; i += 1) {
      const node = interactives[i];
      if (node.getAttribute(HIDDEN_ATTR) === 'true' || containsVideo(node)) {
        continue;
      }
      const text = (node.textContent || '').replace(/\s+/g, ' ').trim().toLowerCase();
      if (!text || text.length > 32) {
        continue;
      }
      if (BLOCKED_TEXT_LABELS.has(text)) {
        hideElementSafely(node);
        continue;
      }
      if (
        NAV_ONLY_TEXT_LABELS.has(text) &&
        node.closest('nav, aside, header, [role="navigation"], [role="menu"]') &&
        !node.closest('[role="dialog"]')
      ) {
        hideElementSafely(node);
      }
    }
  }

  function scheduleHide() {
    if (hideTimer !== null) {
      clearTimeout(hideTimer);
    }
    hideTimer = setTimeout(() => {
      hideTimer = null;
      if (maintenanceFrame) {
        cancelAnimationFrame(maintenanceFrame);
      }
      maintenanceFrame = requestAnimationFrame(() => {
        maintenanceFrame = 0;
        hideUnwantedElements();
      });
    }, DEBOUNCE_MS);
  }

  function svgIcon(path) {
    return '<svg viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="' +
      path + '" /></svg>';
  }

  function createTitlebarButton(className, label, path) {
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'ztik-titlebar-button ' + className;
    button.title = label;
    button.setAttribute('aria-label', label);
    button.innerHTML = svgIcon(path);
    return button;
  }

  function invokeWindowCommand(command) {
    try {
      const internals = window.__TAURI_INTERNALS__;
      if (!internals || typeof internals.invoke !== 'function') {
        return Promise.resolve({ ok: false, value: undefined });
      }
      return Promise.resolve(internals.invoke('plugin:window|' + command))
        .then((value) => ({ ok: true, value }))
        .catch(() => ({ ok: false, value: undefined }));
    } catch (_) {
      return Promise.resolve({ ok: false, value: undefined });
    }
  }

  function updateMaximizeIcon(maximized) {
    const button = document.querySelector('#ztik-titlebar-maximize');
    if (!button) {
      return;
    }
    window.__ztikWindowMaximized = maximized;
    button.title = maximized ? 'Khôi phục' : 'Phóng to';
    button.setAttribute('aria-label', button.title);
    button.innerHTML = svgIcon(maximized ? 'M8 7h11v11H8z M5 17H4V4h13v1' : 'M5 5h14v14H5z');
  }

  function syncMaximizeStateFromViewport() {
    if (!window.screen) {
      return;
    }
    const maximized =
      window.outerWidth >= window.screen.availWidth - 24 &&
      window.outerHeight >= window.screen.availHeight - 48;
    if (Boolean(window.__ztikWindowMaximized) !== maximized) {
      updateMaximizeIcon(maximized);
    }
  }

  function syncMaximizeStateViaIpc() {
    invokeWindowCommand('is_maximized').then((result) => {
      if (result.ok && typeof result.value === 'boolean') {
        if (Boolean(window.__ztikWindowMaximized) !== result.value) {
          updateMaximizeIcon(result.value);
        }
        return;
      }
      syncMaximizeStateFromViewport();
    });
  }

  function callWindowAction(command) {
    return invokeWindowCommand(command).then((result) => result.ok);
  }

  function toggleMaximizeWindow() {
    const nextState = !window.__ztikWindowMaximized;
    return callWindowAction('toggle_maximize').then((ok) => {
      if (ok) {
        updateMaximizeIcon(nextState);
        window.setTimeout(syncMaximizeStateViaIpc, 120);
      }
      return ok;
    });
  }

  function isNativeDecorationsEnabled() {
    return (
      window.__ZTIK_NATIVE_DECORATIONS === true ||
      Boolean(document.documentElement && document.documentElement.classList.contains('ztik-native-decorations'))
    );
  }

  function setNativeDecorationsMode(enabled) {
    const active = Boolean(enabled);
    window.__ZTIK_NATIVE_DECORATIONS = active;
    if (document.documentElement) {
      document.documentElement.classList.toggle('ztik-native-decorations', active);
    }
    const bar = document.getElementById(TITLEBAR_ID);
    if (active) {
      if (bar) {
        bar.style.display = 'none';
      }
      syncViewportLayout();
      return;
    }
    if (bar) {
      bar.style.removeProperty('display');
    } else {
      ensureTitlebar();
    }
    syncViewportLayout();
  }

  function ensureTitlebar() {
    if (isNativeDecorationsEnabled()) {
      const existing = document.getElementById(TITLEBAR_ID);
      if (existing) {
        existing.style.display = 'none';
      }
      return;
    }
    const root = document.body || document.documentElement;
    if (!root) {
      return;
    }
    let bar = document.getElementById(TITLEBAR_ID);
    if (bar && bar.isConnected) {
      bar.style.removeProperty('display');
      return;
    }
    if (!bar) {
      bar = document.createElement('div');
      bar.id = TITLEBAR_ID;
      bar.setAttribute('role', 'toolbar');
      bar.setAttribute('aria-label', 'ztik controls');

      const brand = document.createElement('div');
      brand.className = 'ztik-titlebar-brand';
      brand.textContent = 'ztik';
      bar.appendChild(brand);

      const drag = document.createElement('div');
      drag.className = 'ztik-titlebar-drag';
      drag.setAttribute('aria-hidden', 'true');
      bar.appendChild(drag);

      const actions = document.createElement('div');
      actions.className = 'ztik-titlebar-actions';
      const download = createTitlebarButton('ztik-titlebar-download', 'Tải xuống', 'M12 3v12m0 0 4-4m-4 4-4-4 M5 17v3h14v-3');
      download.addEventListener('click', (event) => {
        event.stopPropagation();
        if (typeof window.__ztikDownloadCurrent === 'function') {
          window.__ztikDownloadCurrent();
        }
      });
      actions.appendChild(download);

      const mini = createTitlebarButton('ztik-titlebar-mini', 'Mini-player', 'M4 5h16v14H4z M8 9h8v6H8z');
      mini.addEventListener('click', (event) => {
        event.stopPropagation();
        if (document.documentElement) {
          document.documentElement.classList.toggle('ztik-mini-ui');
        }
        refreshMiniPlayerMode();
      });
      actions.appendChild(mini);

      const separator = document.createElement('span');
      separator.className = 'ztik-titlebar-separator';
      separator.setAttribute('aria-hidden', 'true');
      actions.appendChild(separator);

      const minimize = createTitlebarButton('ztik-titlebar-minimize', 'Thu nhỏ', 'M5 12h14');
      minimize.addEventListener('click', (event) => {
        event.stopPropagation();
        callWindowAction('minimize');
      });
      actions.appendChild(minimize);

      const maximize = createTitlebarButton('ztik-titlebar-maximize', 'Phóng to', 'M5 5h14v14H5z');
      maximize.id = 'ztik-titlebar-maximize';
      maximize.addEventListener('click', (event) => {
        event.stopPropagation();
        toggleMaximizeWindow();
      });
      actions.appendChild(maximize);

      const close = createTitlebarButton('ztik-titlebar-close', 'Đóng về khay hệ thống', 'M6 6l12 12M18 6 6 18');
      close.addEventListener('click', (event) => {
        event.stopPropagation();
        callWindowAction('close');
      });
      actions.appendChild(close);
      bar.appendChild(actions);

      drag.addEventListener('mousedown', (event) => {
        if (event.button !== 0 || event.detail !== 1) {
          return;
        }
        event.preventDefault();
        callWindowAction('start_dragging');
      });
      drag.addEventListener('dblclick', (event) => {
        event.preventDefault();
        toggleMaximizeWindow();
      });
    }
    root.appendChild(bar);
  }

  function showDownloadToast(message, durationMs) {
    const root = document.body || document.documentElement;
    if (!root) {
      return;
    }
    let toast = document.getElementById(TOAST_ID);
    if (!toast) {
      toast = document.createElement('div');
      toast.id = TOAST_ID;
      toast.setAttribute('role', 'status');
      root.appendChild(toast);
    } else if (!toast.isConnected) {
      root.appendChild(toast);
    }
    toast.textContent = message;
    toast.classList.add('ztik-visible');
    if (toastTimer !== null) {
      clearTimeout(toastTimer);
      toastTimer = null;
    }
    const timeout = typeof durationMs === 'number' ? durationMs : 2400;
    if (timeout > 0) {
      toastTimer = window.setTimeout(() => {
        toast.classList.remove('ztik-visible');
        toastTimer = null;
      }, timeout);
    }
  }

  function findActiveImage() {
    const images = document.querySelectorAll('img');
    let best = null;
    let bestArea = 0;
    for (let i = 0; i < images.length; i += 1) {
      const image = images[i];
      if (!image.complete || image.naturalWidth < 320 || image.naturalHeight < 240) {
        continue;
      }
      const rect = image.getBoundingClientRect();
      const area = Math.max(0, Math.min(rect.right, window.innerWidth) - Math.max(rect.left, 0)) *
        Math.max(0, Math.min(rect.bottom, window.innerHeight) - Math.max(rect.top, 0));
      if (area > bestArea) {
        bestArea = area;
        best = image;
      }
    }
    return best;
  }

  function findCurrentMediaContainer() {
    const video = findActiveVideo();
    if (video) {
      return findVideoItemContainer(video);
    }
    const image = findActiveImage();
    return image
      ? image.closest('article, [data-e2e="recommend-list-item-container"], [data-e2e="browse-video"], [data-e2e="detail-video"]') || image.parentElement
      : null;
  }

  function getCurrentIdentity(container) {
    let id = '';
    let author = '';
    let postType = '';
    const route = (window.location && window.location.pathname || '').match(/^\/@([^/]+)\/(video|photo)\/([^/?]+)/i);
    if (route) {
      author = decodeURIComponent(route[1]);
      postType = route[2].toLowerCase();
      id = route[3];
    }
    if (!id && container) {
      const links = container.querySelectorAll('a[href]');
      for (let i = 0; i < links.length && !id; i += 1) {
        const match = (links[i].getAttribute('href') || '').match(/\/@([^/]+)\/(video|photo)\/([^/?]+)/i);
        if (match) {
          author = decodeURIComponent(match[1]);
          postType = match[2].toLowerCase();
          id = match[3];
        }
      }
    }
    return { id, author: author || 'creator', postType };
  }

  function safeFilenamePart(value) {
    return String(value || 'creator').replace(/[^A-Za-z0-9_-]+/g, '_').replace(/^[_-]+|[_-]+$/g, '').slice(0, 64) || 'creator';
  }

  function extensionFromMedia(contentType, rawUrl, fallback) {
    const normalized = String(contentType || '').split(';')[0].trim().toLowerCase();
    if (normalized === 'image/png') {
      return 'png';
    }
    if (normalized === 'image/webp') {
      return 'webp';
    }
    if (normalized === 'image/jpeg' || normalized === 'image/jpg') {
      return 'jpg';
    }
    if (normalized === 'video/webm') {
      return 'webm';
    }
    if (normalized === 'video/mp4') {
      return 'mp4';
    }
    try {
      const pathname = new URL(rawUrl).pathname.toLowerCase();
      if (pathname.endsWith('.png')) {
        return 'png';
      }
      if (pathname.endsWith('.webp')) {
        return 'webp';
      }
      if (pathname.endsWith('.jpeg') || pathname.endsWith('.jpg')) {
        return 'jpg';
      }
    } catch (_) {}
    return fallback;
  }

  function isTikTokSessionHost(rawUrl) {
    try {
      const urlHost = new URL(rawUrl).hostname.toLowerCase();
      return urlHost === 'tiktok.com' || urlHost.endsWith('.tiktok.com');
    } catch (_) {
      return false;
    }
  }

  function isValidMediaContentType(contentType, kind) {
    const normalized = String(contentType || '').split(';')[0].trim().toLowerCase();
    if (!normalized) {
      return false;
    }
    if (
      normalized.startsWith('text/') ||
      normalized.includes('html') ||
      normalized.includes('json') ||
      normalized.includes('xml')
    ) {
      return false;
    }
    if (normalized === 'application/octet-stream') {
      return true;
    }
    if (kind === 'video') {
      return normalized.startsWith('video/');
    }
    if (kind === 'image') {
      return normalized.startsWith('image/');
    }
    return normalized.startsWith('video/') || normalized.startsWith('image/');
  }

  function collectDomVideoUrls(activeVideo) {
    const urls = [];
    if (!activeVideo) {
      return urls;
    }
    pushUniqueTrustedUrl(urls, activeVideo.currentSrc);
    pushUniqueTrustedUrl(urls, activeVideo.src);
    const sources = activeVideo.querySelectorAll('source[src]');
    for (let i = 0; i < sources.length; i += 1) {
      pushUniqueTrustedUrl(urls, sources[i].getAttribute('src'));
    }
    return urls;
  }

  function collectVideoUrlsFromPerformance(expectedId) {
    try {
      if (!window.performance || typeof window.performance.getEntriesByType !== 'function') {
        return [];
      }
      const entries = window.performance.getEntriesByType('resource');
      const matched = [];
      const fallback = [];
      for (let i = entries.length - 1; i >= 0; i -= 1) {
        const entry = entries[i];
        const safeUrl = trustedMediaUrl(entry && entry.name);
        if (!safeUrl) {
          continue;
        }
        const lower = safeUrl.toLowerCase();
        if (/\.(?:js|css|json|svg|png|jpe?g|webp|gif|woff2?|ico|m3u8|ts)(?:$|[?#])/.test(lower)) {
          continue;
        }
        const looksLikeVideo =
          lower.includes('/video/tos/') ||
          lower.includes('mime_type=video') ||
          /\.mp4(?:$|[?#])/.test(lower) ||
          entry.initiatorType === 'video' ||
          entry.initiatorType === 'media';
        if (!looksLikeVideo) {
          continue;
        }
        const size = Number(entry.decodedBodySize || entry.transferSize || 0);
        if (expectedId && safeUrl.includes(expectedId)) {
          matched.push({ url: safeUrl, size });
        } else {
          fallback.push({ url: safeUrl, size });
        }
      }
      matched.sort((left, right) => right.size - left.size);
      fallback.sort((left, right) => right.size - left.size);
      const urls = [];
      const combined = matched.concat(fallback);
      for (let i = 0; i < combined.length; i += 1) {
        if (!urls.includes(combined[i].url)) {
          urls.push(combined[i].url);
        }
      }
      return urls;
    } catch (_) {
      return [];
    }
  }

  function collectDomSlideshowImages(container) {
    if (!container) {
      return currentImageFallback(container);
    }
    const images = container.querySelectorAll('img');
    const urls = [];
    for (let i = 0; i < images.length; i += 1) {
      const image = images[i];
      if (
        !image.complete ||
        image.naturalWidth < 320 ||
        image.naturalHeight < 240 ||
        image.closest('[data-e2e*="avatar" i]')
      ) {
        continue;
      }
      pushUniqueTrustedUrl(urls, image.currentSrc || image.src);
    }
    return urls.length > 0 ? urls : currentImageFallback(container);
  }

  function currentImageFallback(container) {
    const root = container || document;
    const images = root.querySelectorAll('img');
    let best = null;
    for (let i = 0; i < images.length; i += 1) {
      const image = images[i];
      if (!image.complete || image.naturalWidth < 320 || image.naturalHeight < 240) {
        continue;
      }
      const rect = image.getBoundingClientRect();
      const area = Math.max(0, Math.min(rect.right, window.innerWidth) - Math.max(rect.left, 0)) *
        Math.max(0, Math.min(rect.bottom, window.innerHeight) - Math.max(rect.top, 0));
      const pixels = image.naturalWidth * image.naturalHeight;
      if (area > 0 && (!best || area > best.area || (area === best.area && pixels > best.pixels))) {
        best = { area, pixels, url: image.currentSrc || image.src };
      }
    }
    const bestUrl = best && trustedMediaUrl(best.url);
    return bestUrl ? [bestUrl] : [];
  }

  async function tryFetchMediaCandidate(url, init, kind) {
    try {
      const response = await fetch(url, init);
      if (!response || !response.ok) {
        return null;
      }
      const headerType = response.headers ? response.headers.get('content-type') : '';
      if (!isValidMediaContentType(headerType, kind)) {
        return null;
      }
      const blob = await response.blob();
      if (!blob || blob.size === 0) {
        return null;
      }
      if (blob.type && !isValidMediaContentType(blob.type, kind)) {
        return null;
      }
      return { blob, url, contentType: blob.type || headerType || '' };
    } catch (_) {
      return null;
    }
  }

  async function fetchMediaBlob(candidates, kind) {
    for (let i = 0; i < candidates.length; i += 1) {
      const safeUrl = trustedMediaUrl(candidates[i]);
      if (!safeUrl) {
        continue;
      }
      if (isTikTokSessionHost(safeUrl)) {
        const result = await tryFetchMediaCandidate(
          safeUrl,
          {
            credentials: 'include',
            referrerPolicy: 'strict-origin-when-cross-origin',
          },
          kind
        );
        if (result) {
          return result;
        }
        const omitFallback = await tryFetchMediaCandidate(
          safeUrl,
          {
            mode: 'cors',
            credentials: 'omit',
            referrerPolicy: 'strict-origin-when-cross-origin',
          },
          kind
        );
        if (omitFallback) {
          return omitFallback;
        }
        continue;
      }

      const corsResult = await tryFetchMediaCandidate(
        safeUrl,
        {
          mode: 'cors',
          credentials: 'omit',
          referrerPolicy: 'strict-origin-when-cross-origin',
        },
        kind
      );
      if (corsResult) {
        return corsResult;
      }

      const credentialedResult = await tryFetchMediaCandidate(
        safeUrl,
        {
          mode: 'cors',
          credentials: 'include',
          referrerPolicy: 'strict-origin-when-cross-origin',
        },
        kind
      );
      if (credentialedResult) {
        return credentialedResult;
      }
    }
    return null;
  }

  function triggerBlobDownload(blob, filename) {
    if (!blob || blob.size === 0) {
      return false;
    }
    let blobUrl = URL.createObjectURL(blob);
    blob = null;
    const anchor = document.createElement('a');
    anchor.href = blobUrl;
    anchor.download = filename;
    anchor.rel = 'noopener noreferrer';
    anchor.style.display = 'none';
    (document.body || document.documentElement).appendChild(anchor);
    anchor.click();
    anchor.remove();
    window.setTimeout(() => {
      if (!blobUrl) {
        return;
      }
      try {
        URL.revokeObjectURL(blobUrl);
      } catch (_) {}
      blobUrl = null;
    }, BLOB_REVOKE_DELAY_MS);
    return true;
  }

  async function downloadCurrentMedia() {
    if (downloadInFlight) {
      showDownloadToast('Đang tải media, vui lòng đợi...', 1800);
      return;
    }

    scanHydrationData();
    const activeVideo = findActiveVideo();
    const activeImage = findActiveImage();
    const container = findCurrentMediaContainer();
    const identity = getCurrentIdentity(container);
    const metadata =
      getCachedMedia(identity.id) ||
      extractMediaFromReactNodes(
        [activeVideo, activeImage, container, container && container.parentElement],
        identity.id
      );
    const wantsPhoto =
      identity.postType === 'photo' ||
      Boolean(
        metadata &&
          Array.isArray(metadata.imageSlides) &&
          metadata.imageSlides.length > 0 &&
          (!Array.isArray(metadata.videoUrls) || metadata.videoUrls.length === 0)
      ) ||
      (!activeVideo && Boolean(activeImage));
    const author = safeFilenamePart((metadata && metadata.author) || identity.author);
    const resolvedId = identity.id || (metadata && metadata.id) || '';
    const id = safeFilenamePart(resolvedId || 'media');

    downloadInFlight = true;
    try {
      if (wantsPhoto) {
        const slideGroups =
          metadata && Array.isArray(metadata.imageSlides) && metadata.imageSlides.length
            ? metadata.imageSlides
            : collectDomSlideshowImages(container).map((url) => [url]);

        if (!slideGroups.length) {
          showDownloadToast('Chưa nhận diện được media');
          return;
        }

        const total = slideGroups.length;
        showDownloadToast('Đang tải ' + total + ' ảnh gốc...', 0);

        let savedCount = 0;
        for (let i = 0; i < total; i += 1) {
          if (total > 1) {
            showDownloadToast('Đang tải ảnh ' + (i + 1) + '/' + total + '...', 0);
          }
          let result = await fetchMediaBlob(slideGroups[i], 'image');
          if (!result) {
            continue;
          }
          const number = String(i + 1).padStart(2, '0');
          const ext = extensionFromMedia(result.contentType, result.url, 'jpg');
          let blob = result.blob;
          result.blob = null;
          result = null;
          const ok = triggerBlobDownload(
            blob,
            'ztik_' + author + '_' + id + '_' + number + '.' + ext
          );
          blob = null;
          if (ok) {
            savedCount += 1;
          }
        }

        if (savedCount === 0) {
          showDownloadToast('Ảnh bị bảo vệ hoặc chặn tải từ CDN', 3000);
        } else if (savedCount === total) {
          showDownloadToast(
            total === 1
              ? 'Đã lưu vào Downloads/ztik'
              : 'Đã lưu ' + savedCount + ' ảnh vào Downloads/ztik',
            2600
          );
        } else {
          showDownloadToast('Đã lưu ' + savedCount + '/' + total + ' ảnh vào Downloads/ztik', 3000);
        }
        return;
      }

      const videoCandidates = [];
      if (metadata && Array.isArray(metadata.videoUrls)) {
        for (let i = 0; i < metadata.videoUrls.length; i += 1) {
          pushUniqueTrustedUrl(videoCandidates, metadata.videoUrls[i]);
        }
      }
      const domVideos = collectDomVideoUrls(activeVideo);
      for (let i = 0; i < domVideos.length; i += 1) {
        pushUniqueTrustedUrl(videoCandidates, domVideos[i]);
      }
      const perfVideos = collectVideoUrlsFromPerformance(resolvedId);
      for (let i = 0; i < perfVideos.length; i += 1) {
        pushUniqueTrustedUrl(videoCandidates, perfVideos[i]);
      }

      if (!videoCandidates.length) {
        showDownloadToast('Chưa nhận diện được media');
        return;
      }

      showDownloadToast('Đang tải video gốc...', 0);
      let result = await fetchMediaBlob(videoCandidates, 'video');
      if (!result) {
        showDownloadToast('Video được bảo vệ hoặc bị chặn tải', 3200);
        return;
      }

      const ext = extensionFromMedia(result.contentType, result.url, 'mp4');
      let blob = result.blob;
      result.blob = null;
      result = null;
      triggerBlobDownload(blob, 'ztik_' + author + '_' + id + '.' + ext);
      blob = null;
      showDownloadToast('Đã lưu vào Downloads/ztik', 2600);
    } finally {
      downloadInFlight = false;
    }
  }

  function isElementVisible(el) {
    if (!el || !el.isConnected) {
      return false;
    }
    if (el.getAttribute(HIDDEN_ATTR) === 'true') {
      return false;
    }
    const style = window.getComputedStyle(el);
    if (!style || style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0') {
      return false;
    }
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function visibleAreaInViewport(el) {
    const rect = el.getBoundingClientRect();
    const xOverlap = Math.max(0, Math.min(rect.right, window.innerWidth) - Math.max(rect.left, 0));
    const yOverlap = Math.max(0, Math.min(rect.bottom, window.innerHeight) - Math.max(rect.top, 0));
    return xOverlap * yOverlap;
  }

  function findActiveVideo() {
    const videos = document.querySelectorAll('video');
    let bestVideo = null;
    let bestScore = 0;

    for (let i = 0; i < videos.length; i += 1) {
      const video = videos[i];
      const area = visibleAreaInViewport(video);
      if (area <= 0) {
        continue;
      }
      const playingBonus = !video.paused && !video.ended ? 1.25 : 1;
      const score = area * playingBonus;
      if (score > bestScore) {
        bestScore = score;
        bestVideo = video;
      }
    }
    return bestVideo;
  }

  function getActiveVideoLayer() {
    if (!activeVideoLayerRef) {
      return null;
    }
    const el = typeof activeVideoLayerRef.deref === 'function' ? activeVideoLayerRef.deref() : activeVideoLayerRef;
    if (!el || !el.isConnected) {
      activeVideoLayerRef = null;
      return null;
    }
    return el;
  }

  function markActiveVideoForCompositing(video) {
    if (!video || video.tagName !== 'VIDEO' || !video.isConnected) {
      return;
    }
    restoreNearViewportVideo(video);
    const target = video.closest('[data-e2e="video-player-container"]') || video;
    const prev = getActiveVideoLayer();
    if (prev && prev !== target) {
      prev.removeAttribute('data-ztik-video-layer');
    }
    target.setAttribute('data-ztik-video-layer', '1');
    activeVideoLayerRef = typeof WeakRef === 'function' ? new WeakRef(target) : target;
  }

  function suspendOffscreenVideo(video) {
    if (!video || video.tagName !== 'VIDEO' || !video.isConnected) {
      return;
    }
    if (visibleAreaInViewport(video) > 0) {
      return;
    }
    if (!savedVideoPreload.has(video)) {
      savedVideoPreload.set(video, video.getAttribute('preload'));
    }
    offscreenVideos.add(video);
    if (!video.paused) {
      try {
        video.pause();
      } catch (_) {}
    }
    if (video.preload !== 'none') {
      try {
        video.preload = 'none';
      } catch (_) {}
    }
  }

  function restoreNearViewportVideo(video) {
    if (!video || video.tagName !== 'VIDEO' || !offscreenVideos.has(video)) {
      return;
    }
    offscreenVideos.delete(video);
    if (savedVideoPreload.has(video)) {
      const previous = savedVideoPreload.get(video);
      savedVideoPreload.delete(video);
      try {
        if (previous === null || previous === undefined || previous === '') {
          video.removeAttribute('preload');
        } else {
          video.preload = previous;
        }
      } catch (_) {}
    } else {
      try {
        video.removeAttribute('preload');
      } catch (_) {}
    }
  }

  function classifyFeedCard(el) {
    if (!el || el.nodeType !== 1) {
      return '';
    }
    if (el.matches('[data-e2e="user-post-item"], [data-e2e="explore-item"], [data-e2e="search_top-item"], [data-e2e="challenge-item"]')) {
      return 'grid';
    }
    return 'feed';
  }

  function ensureVideoObserver() {
    if (videoIntersectionObserver || typeof IntersectionObserver === 'undefined') {
      return videoIntersectionObserver;
    }
    videoIntersectionObserver = new IntersectionObserver(
      (entries) => {
        for (let i = 0; i < entries.length; i += 1) {
          const entry = entries[i];
          const video = entry.target;
          if (!video || !video.isConnected) {
            videoIntersectionObserver.unobserve(video);
            continue;
          }
          if (entry.isIntersecting) {
            restoreNearViewportVideo(video);
          } else {
            suspendOffscreenVideo(video);
          }
        }
      },
      { root: null, rootMargin: VIDEO_VIEWPORT_MARGIN, threshold: 0 }
    );
    return videoIntersectionObserver;
  }

  function ensureCardObserver() {
    if (cardIntersectionObserver || typeof IntersectionObserver === 'undefined') {
      return cardIntersectionObserver;
    }
    cardIntersectionObserver = new IntersectionObserver(
      (entries) => {
        for (let i = 0; i < entries.length; i += 1) {
          const entry = entries[i];
          const card = entry.target;
          if (!card || !card.isConnected) {
            cardIntersectionObserver.unobserve(card);
            continue;
          }
          if (entry.isIntersecting) {
            if (card.hasAttribute(OFFSCREEN_CARD_ATTR)) {
              card.removeAttribute(OFFSCREEN_CARD_ATTR);
            }
            const videos = card.querySelectorAll('video');
            for (let j = 0; j < videos.length; j += 1) {
              restoreNearViewportVideo(videos[j]);
            }
            continue;
          }
          if (
            visibleAreaInViewport(card) > 0 ||
            (document.activeElement && card.contains(document.activeElement)) ||
            card.querySelector('[role="dialog"], [aria-modal="true"]')
          ) {
            continue;
          }
          const height = Math.round(entry.boundingClientRect ? entry.boundingClientRect.height : 0);
          if (height >= 120) {
            card.style.setProperty('--ztik-cv-h', height + 'px');
          }
          const kind = classifyFeedCard(card);
          if (kind && card.getAttribute(OFFSCREEN_CARD_ATTR) !== kind) {
            card.setAttribute(OFFSCREEN_CARD_ATTR, kind);
          }
          const videos = card.querySelectorAll('video');
          for (let j = 0; j < videos.length; j += 1) {
            suspendOffscreenVideo(videos[j]);
          }
        }
      },
      { root: null, rootMargin: CARD_VIEWPORT_MARGIN, threshold: 0 }
    );
    return cardIntersectionObserver;
  }

  function observeFeedMedia() {
    const vObserver = ensureVideoObserver();
    if (vObserver) {
      const videos = document.querySelectorAll('video');
      for (let i = 0; i < videos.length; i += 1) {
        const video = videos[i];
        if (!observedVideos.has(video)) {
          observedVideos.add(video);
          vObserver.observe(video);
        }
      }
    }

    const cObserver = ensureCardObserver();
    if (cObserver) {
      const cards = document.querySelectorAll(
        'article, [data-e2e="recommend-list-item-container"], [data-e2e="user-post-item"], ' +
        '[data-e2e="explore-item"], [data-e2e="search_top-item"], [data-e2e="challenge-item"]'
      );
      for (let i = 0; i < cards.length; i += 1) {
        const card = cards[i];
        if (
          observedCards.has(card) ||
          isZtikOwnElement(card) ||
          card === document.body ||
          card === document.documentElement ||
          card.tagName === 'MAIN'
        ) {
          continue;
        }
        observedCards.add(card);
        cObserver.observe(card);
      }
    }
  }

  function freeMemoryInPage() {
    mediaCache.clear();
    const keys = Object.keys(lastScriptFingerprints);
    for (let i = 0; i < keys.length; i += 1) {
      delete lastScriptFingerprints[keys[i]];
    }
    const prevLayer = getActiveVideoLayer();
    if (prevLayer) {
      prevLayer.removeAttribute('data-ztik-video-layer');
    }
    activeVideoLayerRef = null;
    try {
      const videos = document.querySelectorAll('video');
      for (let i = 0; i < videos.length; i += 1) {
        try {
          videos[i].pause();
          videos[i].preload = 'none';
        } catch (_) {}
      }
    } catch (_) {}
    try {
      if (window.performance && typeof window.performance.clearResourceTimings === 'function') {
        window.performance.clearResourceTimings();
      }
    } catch (_) {}
  }

  function findVideoItemContainer(video) {
    if (!video) {
      return null;
    }
    const direct = video.closest(
      'article, [data-e2e="recommend-list-item-container"], [data-e2e="browse-video"], [data-e2e="detail-video"]'
    );
    if (direct) {
      return direct;
    }

    let current = video.parentElement;
    let depth = 0;
    while (current && current !== document.body && depth < 10) {
      if (
        current.querySelector(
          '[data-e2e*="repost" i], [data-e2e*="share" i], [data-e2e="like-icon"], [data-e2e="browse-like-icon"]'
        )
      ) {
        return current;
      }
      current = current.parentElement;
      depth += 1;
    }
    return video.parentElement;
  }

  function isRepostTab(el) {
    if (!el) {
      return true;
    }
    if (el.getAttribute('role') === 'tab' || el.closest('[role="tab"], [role="tablist"]')) {
      return true;
    }
    const e2e = (el.getAttribute('data-e2e') || '').toLowerCase();
    return e2e.includes('tab');
  }

  function findVisibleRepostControl(scope) {
    const root = scope || document;
    const candidates = root.querySelectorAll('[data-e2e*="repost" i]');
    let best = null;
    let bestArea = 0;

    for (let i = 0; i < candidates.length; i += 1) {
      const el = candidates[i];
      if (isRepostTab(el)) {
        continue;
      }
      const clickable = el.closest('button, [role="button"], [role="menuitem"], a') || el;
      if (isRepostTab(clickable) || !isElementVisible(clickable)) {
        continue;
      }
      const area = visibleAreaInViewport(clickable);
      if (area > bestArea) {
        bestArea = area;
        best = clickable;
      }
    }
    return best;
  }

  function triggerRepost() {
    const activeVideo = findActiveVideo();
    const container = findVideoItemContainer(activeVideo);

    const directControl =
      (container && findVisibleRepostControl(container)) || findVisibleRepostControl(document);
    if (directControl) {
      directControl.click();
      return true;
    }

    if (!container) {
      return false;
    }

    const shareTarget = container.querySelector(
      '[data-e2e="share-btn"], [data-e2e="video-share"], [data-e2e="share-icon"], [data-e2e="browse-share"]'
    );
    const shareButton =
      (shareTarget && (shareTarget.closest('button, [role="button"]') || shareTarget)) || null;
    if (!shareButton || !isElementVisible(shareButton)) {
      return false;
    }

    shareButton.dispatchEvent(new MouseEvent('mouseenter', { bubbles: true }));
    shareButton.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));

    setTimeout(() => {
      const menuRepost = findVisibleRepostControl(document);
      if (menuRepost) {
        menuRepost.click();
      }
    }, 150);

    return true;
  }

  function triggerNextVideo() {
    const nextButtonSelectors = [
      'button[data-e2e="arrow-right"]',
      'button[data-e2e="browse-next"]',
      'button[data-e2e="arrow-down"]',
      'button[data-e2e="feed-arrow-down"]',
      '[data-e2e="arrow-right"]',
      '[data-e2e="browse-next"]',
      'button[aria-label*="next video" i]',
      'button[aria-label="Go to next video"]',
    ];

    for (let i = 0; i < nextButtonSelectors.length; i += 1) {
      const btn = document.querySelector(nextButtonSelectors[i]);
      if (
        btn &&
        isElementVisible(btn) &&
        !btn.disabled &&
        btn.getAttribute('aria-disabled') !== 'true'
      ) {
        btn.click();
        return true;
      }
    }

    const activeVideo = findActiveVideo();
    const activeItem = findVideoItemContainer(activeVideo);
    const items = Array.from(
      document.querySelectorAll('article, [data-e2e="recommend-list-item-container"]')
    );

    if (items.length > 0) {
      const currentTop = activeItem ? activeItem.getBoundingClientRect().top : 0;
      const nextItem = items.find((item) => {
        if (item === activeItem) {
          return false;
        }
        return item.getBoundingClientRect().top > currentTop + 20;
      });
      if (nextItem) {
        nextItem.scrollIntoView({ behavior: 'smooth', block: 'center' });
        return true;
      }
    }

    const scrollStep = Math.max(300, Math.round(window.innerHeight * 0.85));
    window.scrollBy({ top: scrollStep, behavior: 'smooth' });
    return true;
  }

  function isEditableElement(el) {
    if (!el || el.nodeType !== 1) {
      return false;
    }
    const tag = el.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') {
      return true;
    }
    if (el.isContentEditable) {
      return true;
    }
    if (
      el.closest(
        'input, textarea, select, [contenteditable="true"], [contenteditable=""], [role="textbox"], [data-e2e="comment-input"]'
      )
    ) {
      return true;
    }
    return false;
  }

  function onKeyDown(event) {
    if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) {
      return;
    }
    if (isEditableElement(event.target) || isEditableElement(document.activeElement)) {
      return;
    }

    const pathname = (window.location && window.location.pathname || '').toLowerCase();
    if (pathname.startsWith('/messages')) {
      return;
    }

    const key = event.key;
    if (key === 'r' || key === 'R') {
      if (triggerRepost()) {
        event.preventDefault();
        event.stopPropagation();
      }
      return;
    }

    if (key === 'n' || key === 'N' || key === 'ArrowDown') {
      if (triggerNextVideo()) {
        event.preventDefault();
        event.stopPropagation();
      }
    }
  }

  function onVisibilityChange() {
    const videos = document.querySelectorAll('video');
    if (document.hidden) {
      for (let i = 0; i < videos.length; i += 1) {
        const video = videos[i];
        if (!video.paused && !video.ended) {
          ztikPausedVideos.add(video);
          try {
            video.pause();
          } catch (_) {
            // Ignore pause errors on detached or uninitialized media elements.
          }
        }
      }
      return;
    }

    for (let i = 0; i < videos.length; i += 1) {
      const video = videos[i];
      if (ztikPausedVideos.has(video)) {
        ztikPausedVideos.delete(video);
        if (video.paused && !video.ended && visibleAreaInViewport(video) > 0) {
          try {
            const playResult = video.play();
            if (playResult && typeof playResult.catch === 'function') {
              playResult.catch(() => {});
            }
          } catch (_) {
            // Ignore autoplay restriction errors.
          }
        }
      }
    }
  }

  function hookSpaNavigation() {
    const origPushState = history.pushState;
    if (typeof origPushState === 'function') {
      history.pushState = function () {
        const result = origPushState.apply(this, arguments);
        scheduleHide();
        return result;
      };
    }

    const origReplaceState = history.replaceState;
    if (typeof origReplaceState === 'function') {
      history.replaceState = function () {
        const result = origReplaceState.apply(this, arguments);
        scheduleHide();
        return result;
      };
    }

    window.addEventListener('popstate', () => {
      scheduleHide();
    }, { passive: true });
  }

  function startObserver() {
    const target = document.documentElement;
    if (!target || typeof MutationObserver === 'undefined') {
      return;
    }
    const observer = new MutationObserver(() => {
      scheduleHide();
    });
    observer.observe(target, { childList: true, subtree: true });

    const html = document.documentElement;
    if (html) {
      const classObserver = new MutationObserver(() => {
        scheduleHide();
      });
      classObserver.observe(html, { attributes: true, attributeFilter: ['class'] });
    }
  }

  function init() {
    ensureStyle();
    scanHydrationData();
    if (typeof window.__ZTIK_MINI !== 'boolean') {
      window.__ZTIK_MINI = false;
    }
    window.__ztikDownloadCurrent = downloadCurrentMedia;
    window.__ztikRefreshMiniPlayer = refreshMiniPlayerMode;
    window.__ztikSetNativeDecorations = setNativeDecorationsMode;
    window.__ztikFreeMemory = freeMemoryInPage;
    window.__ztikShowToast = showDownloadToast;
    refreshMiniPlayerMode();
    markActiveVideoForCompositing(findActiveVideo());
    hideUnwantedElements();
    ensureTitlebar();
    syncMaximizeStateFromViewport();
    startObserver();
    hookSpaNavigation();
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('resize', () => {
      syncMaximizeStateFromViewport();
      requestAnimationFrame(syncViewportLayout);
    }, { passive: true });
    document.addEventListener('play', (event) => markActiveVideoForCompositing(event.target), true);
    document.addEventListener(
      'contentvisibilityautostatechange',
      (event) => {
        const target = event && event.target;
        if (!target || target.nodeType !== 1 || !target.hasAttribute(OFFSCREEN_CARD_ATTR)) {
          return;
        }
        const videos = target.querySelectorAll('video');
        for (let i = 0; i < videos.length; i += 1) {
          if (event.skipped) {
            suspendOffscreenVideo(videos[i]);
          } else {
            restoreNearViewportVideo(videos[i]);
          }
        }
      },
      { capture: true }
    );
    document.addEventListener('visibilitychange', onVisibilityChange);
    if (typeof window.__ztikPendingToast === 'string' && window.__ztikPendingToast) {
      const pendingMessage = window.__ztikPendingToast;
      window.__ztikPendingToast = '';
      showDownloadToast(pendingMessage, 2600);
    }
  }

  if (document.readyState === 'loading') {
    ensureStyle();
    document.addEventListener('DOMContentLoaded', init, { once: true });
  } else {
    init();
  }
})();
