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
  const HIDDEN_ATTR = 'data-ztik-hidden';
  const DEBOUNCE_MS = 120;

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
  let hideTimer = null;

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
    if (window.__ZTIK_MINI !== true) {
      for (let i = 0; i < marked.length; i += 1) {
        marked[i].removeAttribute('data-ztik-mini-hide');
      }
      return;
    }

    const html = document.documentElement;
    if (html && !html.classList.contains('ztik-mini')) {
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

  function hideUnwantedElements() {
    ensureStyle();
    refreshMiniPlayerMode();

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
      hideUnwantedElements();
    }, DEBOUNCE_MS);
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
        refreshMiniPlayerMode();
        scheduleHide();
        return result;
      };
    }

    const origReplaceState = history.replaceState;
    if (typeof origReplaceState === 'function') {
      history.replaceState = function () {
        const result = origReplaceState.apply(this, arguments);
        refreshMiniPlayerMode();
        scheduleHide();
        return result;
      };
    }

    window.addEventListener('popstate', () => {
      refreshMiniPlayerMode();
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
        refreshMiniPlayerMode();
      });
      classObserver.observe(html, { attributes: true, attributeFilter: ['class'] });
    }
  }

  function init() {
    ensureStyle();
    if (typeof window.__ZTIK_MINI !== 'boolean') {
      window.__ZTIK_MINI = false;
    }
    window.__ztikRefreshMiniPlayer = refreshMiniPlayerMode;
    refreshMiniPlayerMode();
    hideUnwantedElements();
    startObserver();
    hookSpaNavigation();
    window.addEventListener('keydown', onKeyDown, true);
    document.addEventListener('visibilitychange', onVisibilityChange);
  }

  if (document.readyState === 'loading') {
    ensureStyle();
    document.addEventListener('DOMContentLoaded', init, { once: true });
  } else {
    init();
  }
})();
