// Background Service Worker: Download Cepat WebExtension (Manifest V3)

const NATIVE_HOST = "com.downloadcepat.nativehost";
const INTERCEPT_EXTENSIONS = [
  '.zip', '.rar', '.7z', '.tar', '.gz', '.bz2', '.xz', '.tgz', '.iso', '.img', '.bin', '.001', '.torrent',
  '.exe', '.msi', '.msix', '.dmg', '.pkg', '.deb', '.rpm', '.appimage', '.apk', '.xapk', '.ipa', '.obb', '.run',
  '.mp4', '.mkv', '.avi', '.mov', '.webm', '.flv', '.m4v', '.m3u8', '.mpd',
  '.pdf', '.epub', '.doc', '.docx', '.xls', '.xlsx', '.ppt', '.pptx', '.odt', '.ods', '.rtf', '.cbz', '.cbr',
  '.mp3', '.flac', '.wav', '.m4a', '.aac'
];

const INTERCEPT_MIME_PREFIXES = [
  "application/zip",
  "application/x-zip",
  "application/x-rar",
  "application/vnd.rar",
  "application/x-7z",
  "application/x-tar",
  "application/gzip",
  "application/x-gzip",
  "application/x-bzip2",
  "application/x-xz",
  "application/x-iso9660-image",
  "application/x-apple-diskimage",
  "application/x-msdownload",
  "application/x-msi",
  "application/vnd.android.package-archive",
  "application/x-bittorrent",
  "application/pdf",
  "application/mp2t",
  "video/",
  "audio/"
];

let isInterceptionEnabled = true;
let excludedDomains = [];
let currentExtLang = "en";
const tabMediaStreams = new Map(); // tabId -> Array<{ url, mimeType, fileSize, label }>

const BG_I18N = {
  en: {
    singleLinkTitle: "Download with Download Cepat ⚡",
    allLinksTitle: "Download All Links on This Page ⚡",
    noActiveTab: "Active tab not found",
    noLinksFound: "No downloadable links found on this page.",
    desktopNoResp: "Download Cepat desktop app did not respond."
  },
  id: {
    singleLinkTitle: "Download dengan Download Cepat ⚡",
    allLinksTitle: "Download Semua Link di Halaman Ini ⚡",
    noActiveTab: "Tab aktif tidak ditemukan",
    noLinksFound: "Tidak ada link unduhan yang ditemukan di halaman ini.",
    desktopNoResp: "Aplikasi Download Cepat desktop belum merespons."
  },
  ms: {
    singleLinkTitle: "Muat Turun dengan Download Cepat ⚡",
    allLinksTitle: "Muat Turun Semua Pautan di Halaman Ini ⚡",
    noActiveTab: "Tab aktif tidak ditemui",
    noLinksFound: "Tiada pautan muat turun ditemui di halaman ini.",
    desktopNoResp: "Aplikasi desktop Download Cepat tidak memberi respons."
  },
  zh: {
    singleLinkTitle: "使用 Download Cepat 下载 ⚡",
    allLinksTitle: "下载此页面所有链接 ⚡",
    noActiveTab: "未找到活动标签页",
    noLinksFound: "此页面未找到可下载的链接。",
    desktopNoResp: "Download Cepat 桌面应用未响应。"
  },
  ja: {
    singleLinkTitle: "Download Cepat でダウンロード ⚡",
    allLinksTitle: "このページの全リンクをダウンロード ⚡",
    noActiveTab: "アクティブタブが見つかりません",
    noLinksFound: "このページにダウンロード可能なリンクが見つかりません。",
    desktopNoResp: "デスクトップ版 Download Cepat が応答しません。"
  },
  ko: {
    singleLinkTitle: "Download Cepat으로 다운로드 ⚡",
    allLinksTitle: "이 페이지의 모든 링크 다운로드 ⚡",
    noActiveTab: "활성 탭을 찾을 수 없습니다",
    noLinksFound: "이 페이지에서 다운로드 가능한 링크를 찾지 못했습니다.",
    desktopNoResp: "Download Cepat 데스크톱 앱이 응답하지 않습니다."
  },
  es: {
    singleLinkTitle: "Descargar con Download Cepat ⚡",
    allLinksTitle: "Descargar todos los enlaces de esta página ⚡",
    noActiveTab: "No se encontró pestaña activa",
    noLinksFound: "No se encontraron enlaces descargables en esta página.",
    desktopNoResp: "La aplicación de escritorio Download Cepat no respondió."
  },
  fr: {
    singleLinkTitle: "Télécharger avec Download Cepat ⚡",
    allLinksTitle: "Télécharger tous les liens de cette page ⚡",
    noActiveTab: "Aucun onglet actif trouvé",
    noLinksFound: "Aucun lien téléchargeable trouvé sur cette page.",
    desktopNoResp: "L'application de bureau Download Cepat n'a pas répondu."
  },
  de: {
    singleLinkTitle: "Mit Download Cepat herunterladen ⚡",
    allLinksTitle: "Alle Links dieser Seite herunterladen ⚡",
    noActiveTab: "Kein aktiver Tab gefunden",
    noLinksFound: "Keine herunterladbaren Links auf dieser Seite gefunden.",
    desktopNoResp: "Die Desktop-App Download Cepat hat nicht geantwortet."
  },
  pt: {
    singleLinkTitle: "Baixar com Download Cepat ⚡",
    allLinksTitle: "Baixar todos os links desta página ⚡",
    noActiveTab: "Aba ativa não encontrada",
    noLinksFound: "Nenhum link para download encontrado nesta página.",
    desktopNoResp: "O aplicativo Download Cepat não respondeu."
  },
  ru: {
    singleLinkTitle: "Скачать через Download Cepat ⚡",
    allLinksTitle: "Скачать все ссылки на странице ⚡",
    noActiveTab: "Активная вкладка не найдена",
    noLinksFound: "На этой странице не найдено ссылок для скачивания.",
    desktopNoResp: "Десктопное приложение Download Cepat не отвечает."
  },
  ar: {
    singleLinkTitle: "تنزيل عبر Download Cepat ⚡",
    allLinksTitle: "تنزيل جميع الروابط في هذه الصفحة ⚡",
    noActiveTab: "لم يتم العثور على علامة تبويب نشطة",
    noLinksFound: "لم يتم العثور على روابط قابلة للتنزيل في هذه الصفحة.",
    desktopNoResp: "تطبيق سطح المكتب Download Cepat لم يستجب."
  },
  hi: {
    singleLinkTitle: "Download Cepat से डाउनलोड करें ⚡",
    allLinksTitle: "इस पृष्ठ के सभी लिंक डाउनलोड करें ⚡",
    noActiveTab: "सक्रिय टैब नहीं मिला",
    noLinksFound: "इस पृष्ठ पर कोई डाउनलोड लिंक नहीं मिला।",
    desktopNoResp: "Download Cepat डेस्कटॉप ऐप ने उत्तर नहीं दिया।"
  },
  vi: {
    singleLinkTitle: "Tải xuống bằng Download Cepat ⚡",
    allLinksTitle: "Tải tất cả liên kết trên trang này ⚡",
    noActiveTab: "Không tìm thấy tab đang hoạt động",
    noLinksFound: "Không tìm thấy liên kết tải xuống nào trên trang này.",
    desktopNoResp: "Ứng dụng máy tính Download Cepat không phản hồi."
  },
  th: {
    singleLinkTitle: "ดาวน์โหลดด้วย Download Cepat ⚡",
    allLinksTitle: "ดาวน์โหลดลิงก์ทั้งหมดในหน้านี้ ⚡",
    noActiveTab: "ไม่พบแท็บที่ใช้งานอยู่",
    noLinksFound: "ไม่พบลิงก์ที่ดาวน์โหลดได้ในหน้านี้",
    desktopNoResp: "แอปเดสก์ท็อป Download Cepat ไม่ตอบสนอง"
  },
  tr: {
    singleLinkTitle: "Download Cepat ile İndir ⚡",
    allLinksTitle: "Bu Sayfadaki Tüm Bağlantıları İndir ⚡",
    noActiveTab: "Etkin sekme bulunamadı",
    noLinksFound: "Bu sayfada indirilebilir bağlantı bulunamadı.",
    desktopNoResp: "Download Cepat masaüstü uygulaması yanıt vermedi."
  }
};

function getBgDict() {
  return BG_I18N[currentExtLang] || BG_I18N.en;
}

function syncContextMenusLanguage() {
  const d = getBgDict();
  try {
    chrome.contextMenus.update("download-cepat-link", { title: d.singleLinkTitle }, () => {
      void chrome.runtime.lastError;
    });
    chrome.contextMenus.update("download-cepat-all-links", { title: d.allLinksTitle }, () => {
      void chrome.runtime.lastError;
    });
  } catch (_) {}
}

chrome.storage.local.get(["dcLang"], (res) => {
  if (res && BG_I18N[res.dcLang]) {
    currentExtLang = res.dcLang;
    syncContextMenusLanguage();
  }
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes.dcLang && BG_I18N[changes.dcLang.newValue]) {
    currentExtLang = changes.dcLang.newValue;
    syncContextMenusLanguage();
  }
});

function normalizeHost(rawUrl) {
  try {
    return new URL(rawUrl).hostname.toLowerCase().replace(/^www\./, "");
  } catch (_) {
    return "";
  }
}

function isDomainExcluded(url) {
  const host = normalizeHost(url);
  if (!host) return false;
  return excludedDomains.some((d) => host === d || host.endsWith("." + d));
}

function loadExtensionSettings() {
  chrome.storage.local.get(["dcEnabled", "dcExcludedDomains"], (res) => {
    if (typeof res.dcEnabled === "boolean") {
      isInterceptionEnabled = res.dcEnabled;
    }
    if (Array.isArray(res.dcExcludedDomains)) {
      excludedDomains = res.dcExcludedDomains;
    }
  });
}

loadExtensionSettings();
chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  if (changes.dcEnabled) {
    isInterceptionEnabled = Boolean(changes.dcEnabled.newValue);
  }
  if (changes.dcExcludedDomains) {
    excludedDomains = Array.isArray(changes.dcExcludedDomains.newValue)
      ? changes.dcExcludedDomains.newValue
      : [];
  }
});

// Passive Network Media Sniffer (MV3 compliant)
function isNonMediaMimeForTs(contentType) {
  const mime = (contentType || "").split(";")[0].trim().toLowerCase();
  if (!mime) return false;
  return (
    mime.startsWith("text/") ||
    mime.startsWith("image/") ||
    mime.startsWith("font/") ||
    mime.includes("javascript") ||
    mime.includes("typescript") ||
    mime === "application/json" ||
    mime === "application/xml"
  );
}

function normalizeSniffedMime(pathLower, contentType) {
  const mime = (contentType || "").split(";")[0].trim().toLowerCase();
  if (mime === "application/mp2t") return "video/mp2t";
  if (mime && mime !== "application/octet-stream") return mime;
  if (pathLower.endsWith(".ts")) return "video/mp2t";
  if (pathLower.endsWith(".mp3")) return "audio/mpeg";
  if (pathLower.endsWith(".m4a")) return "audio/mp4";
  if (pathLower.endsWith(".aac")) return "audio/aac";
  if (pathLower.endsWith(".flac")) return "audio/flac";
  if (pathLower.endsWith(".wav")) return "audio/wav";
  if (pathLower.endsWith(".webm")) return "video/webm";
  if (pathLower.endsWith(".mov")) return "video/quicktime";
  return "video/mp4";
}

if (chrome.webRequest && chrome.webRequest.onResponseStarted) {
  chrome.webRequest.onResponseStarted.addListener(
    (details) => {
      if (!details || details.tabId < 0) return;
      if (details.type === "script" || details.type === "stylesheet") return;
      const u = details.url || "";
      if (!u.startsWith("http://") && !u.startsWith("https://")) return;
      if (u.startsWith("http://127.0.0.1:18291")) return;
      // Skip YouTube/MSE chunked range fragments (resolved via /inspect full formats instead)
      if (u.includes("googlevideo.com/videoplayback")) return;

      let contentType = "";
      let contentLength = null;
      for (const h of details.responseHeaders || []) {
        const name = (h.name || "").toLowerCase();
        if (name === "content-type") contentType = (h.value || "").toLowerCase();
        if (name === "content-length") {
          const parsed = parseInt(h.value || "0", 10);
          if (parsed > 0) contentLength = parsed;
        }
      }

      let pathLower = u.split("?")[0].split("#")[0].toLowerCase();
      try {
        pathLower = new URL(u).pathname.toLowerCase();
      } catch (_) {}

      const isTsUrl = pathLower.endsWith(".ts");
      if (isTsUrl && isNonMediaMimeForTs(contentType)) {
        return;
      }
      // Skip tiny HLS .ts fragments when sniffing streams to keep list crisp
      if (isTsUrl && contentLength !== null && contentLength < 5 * 1024 * 1024) {
        return;
      }

      const existingList = tabMediaStreams.get(details.tabId) || [];
      const tabHasManifest = existingList.some((item) => {
        const itemMime = (item.mimeType || "").toLowerCase();
        return (
          item.url.includes(".m3u8") ||
          item.url.includes(".mpd") ||
          itemMime.includes("mpegurl") ||
          itemMime.includes("dash")
        );
      });
      // Chunked HLS .ts segments without Content-Length should not flood/evict an existing HLS manifest
      if (isTsUrl && contentLength === null && tabHasManifest) {
        return;
      }

      const mimeOnly = contentType.split(";")[0].trim().toLowerCase();
      const isManifest =
        pathLower.endsWith(".m3u8") ||
        pathLower.endsWith(".mpd") ||
        contentType.includes("mpegurl") ||
        contentType.includes("dash+xml");
      const isLargeDirectTs =
        isTsUrl &&
        contentLength !== null &&
        contentLength >= 5 * 1024 * 1024;
      const isStreamUrl =
        isManifest ||
        isLargeDirectTs ||
        pathLower.endsWith(".mp4") ||
        pathLower.endsWith(".webm") ||
        pathLower.endsWith(".mov") ||
        pathLower.endsWith(".m4v") ||
        pathLower.endsWith(".flv") ||
        pathLower.endsWith(".mp3") ||
        pathLower.endsWith(".m4a") ||
        pathLower.endsWith(".aac") ||
        pathLower.endsWith(".flac") ||
        pathLower.endsWith(".wav");
      const isStreamMime =
        contentType.startsWith("video/") ||
        contentType.startsWith("audio/") ||
        mimeOnly === "application/mp2t";

      if (!isStreamUrl && !isStreamMime) return;
      // Skip small UI sound effects (e.g. YouTube voice-search chimes < 64 KB)
      if (!isManifest && contentLength !== null && contentLength < 64 * 1024) {
        return;
      }

      const list = existingList;
      if (!list.some((item) => item.url === u)) {
        let label = pathLower.split("/").pop() || "media";
        if (pathLower.endsWith(".m3u8") || contentType.includes("mpegurl")) {
          label = `HLS Stream (${label})`;
        } else if (pathLower.endsWith(".mpd") || contentType.includes("dash+xml")) {
          label = `DASH Stream (${label})`;
        } else if (isTsUrl || mimeOnly === "video/mp2t" || mimeOnly === "application/mp2t") {
          label = `TS Video (${label})`;
        }
        list.unshift({
          url: u,
          mimeType: normalizeSniffedMime(pathLower, contentType),
          fileSize: contentLength,
          label,
        });
        if (list.length > 25) list.length = 25;
        tabMediaStreams.set(details.tabId, list);
      }
    },
    { urls: ["<all_urls>"] },
    ["responseHeaders"]
  );
}

if (chrome.tabs) {
  chrome.tabs.onRemoved.addListener((tabId) => {
    tabMediaStreams.delete(tabId);
  });
  chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
    if (changeInfo.status === "loading" && changeInfo.url) {
      tabMediaStreams.delete(tabId);
    }
  });
}

function matchesDownloadExtension(targetUrl, filename) {
  const urlLower = (targetUrl || "").toLowerCase();
  const filenameLower = (filename || "").toLowerCase();
  let urlPath = urlLower;
  try {
    urlPath = new URL(targetUrl).pathname.toLowerCase();
  } catch (_) {}

  return INTERCEPT_EXTENSIONS.some(
    (ext) =>
      urlPath.endsWith(ext) ||
      filenameLower.endsWith(ext) ||
      urlLower.includes(ext + "?") ||
      urlLower.includes(ext + "&")
  );
}

function isLargeTsDownload(item) {
  if (typeof item.fileSize !== "number" || item.fileSize < 5 * 1024 * 1024) {
    return false;
  }
  const filenameLower = (item && item.filename ? item.filename : "").toLowerCase();
  const targetUrl = (item && (item.finalUrl || item.url)) || "";
  let urlPath = targetUrl.toLowerCase();
  try {
    urlPath = new URL(targetUrl).pathname.toLowerCase();
  } catch (_) {}
  return filenameLower.endsWith(".ts") || urlPath.endsWith(".ts");
}

function matchesDownloadMime(item) {
  const mime = (item && item.mime ? item.mime : "").toLowerCase().split(";")[0].trim();
  if (!mime) {
    return isLargeTsDownload(item);
  }
  if (INTERCEPT_MIME_PREFIXES.some((prefix) => mime.startsWith(prefix))) {
    return true;
  }
  if (mime === "application/octet-stream" && typeof item.fileSize === "number" && item.fileSize >= 1048576) {
    return true;
  }
  return false;
}

async function resolveEffectiveReferrer(referrer) {
  if (referrer && (referrer.startsWith("http://") || referrer.startsWith("https://"))) {
    return referrer;
  }
  try {
    const tabs = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    if (tabs && tabs[0] && tabs[0].url && (tabs[0].url.startsWith("http://") || tabs[0].url.startsWith("https://"))) {
      return tabs[0].url;
    }
  } catch (_) {}
  return "";
}

async function collectCookiesHeader(targetUrl, referrerUrl, extraUrls = []) {
  if (!chrome.cookies || !chrome.cookies.getAll) {
    return "";
  }
  try {
    const cookieMap = new Map();
    const urlsToCheck = [];
    const seenCheckUrls = new Set();
    const pushUrl = (raw) => {
      if (raw && (raw.startsWith("http://") || raw.startsWith("https://")) && !seenCheckUrls.has(raw)) {
        seenCheckUrls.add(raw);
        urlsToCheck.push(raw);
      }
    };
    const pushWithRoot = (raw) => {
      pushUrl(raw);
      if (raw && (raw.startsWith("http://") || raw.startsWith("https://"))) {
        try {
          const u = new URL(raw);
          const hostParts = u.hostname.split(".");
          if (hostParts.length > 2) {
            const rootDomain = hostParts.slice(-2).join(".");
            pushUrl(`${u.protocol}//${rootDomain}/`);
          }
        } catch (_) {}
      }
    };

    pushWithRoot(targetUrl);
    pushUrl(referrerUrl);
    for (const extra of Array.isArray(extraUrls) ? extraUrls : []) {
      pushUrl(extra);
    }

    for (const checkUrl of urlsToCheck) {
      const cookies = await chrome.cookies.getAll({ url: checkUrl });
      for (const c of cookies) {
        if (!cookieMap.has(c.name)) {
          cookieMap.set(c.name, c.value);
        }
      }
    }

    if (cookieMap.size === 0) return "";
    return Array.from(cookieMap.entries())
      .map(([name, value]) => `${name}=${value}`)
      .join("; ");
  } catch (err) {
    console.warn("[Download Cepat] Gagal mengambil cookie sesi:", err);
    return "";
  }
}

// Intercept browser download requests
chrome.downloads.onDeterminingFilename.addListener((item, suggest) => {
  if (!isInterceptionEnabled) {
    return false;
  }

  const targetUrl = item.finalUrl || item.url || "";
  if (isDomainExcluded(targetUrl) || isDomainExcluded(item.referrer || "")) {
    return false;
  }
  // Do not intercept local blobs/data URLs generated in-page
  if (!targetUrl.startsWith("http://") && !targetUrl.startsWith("https://")) {
    return false;
  }

  if (!matchesDownloadExtension(targetUrl, item.filename) && !matchesDownloadMime(item)) {
    return false;
  }

  (async () => {
    try {
      const handled = await sendToDownloadCepat(
        targetUrl,
        item.filename,
        item.referrer || "",
        item.fileSize,
        item.mime
      );

      if (handled) {
        chrome.downloads.cancel(item.id, () => {
          chrome.downloads.erase({ id: item.id });
        });
      } else {
        suggest();
      }
    } catch (err) {
      console.error("[Download Cepat] Gagal mengalihkan unduhan:", err);
      suggest();
    }
  })();

  return true;
});

// Context Menus: single download & Download All Links on Page
chrome.runtime.onInstalled.addListener(() => {
  chrome.storage.local.get(["dcLang"], (res) => {
    if (res && BG_I18N[res.dcLang]) {
      currentExtLang = res.dcLang;
    }
    const d = getBgDict();
    chrome.contextMenus.removeAll(() => {
      chrome.contextMenus.create({
        id: "download-cepat-link",
        title: d.singleLinkTitle,
        contexts: ["link", "video", "audio"]
      });
      chrome.contextMenus.create({
        id: "download-cepat-all-links",
        title: d.allLinksTitle,
        contexts: ["page", "selection"]
      });
    });
  });
});

async function collectAndSendAllPageLinks(tab) {
  const d = getBgDict();
  if (!tab || !tab.id) return { ok: false, error: d.noActiveTab };
  try {
    const collectDomLinks = () => {
      const seen = new Set();
      const items = [];
      const addUrl = (raw, label) => {
        if (!raw) return;
        const rawTrimmed = String(raw).trim();
        if (/^magnet:\?xt=/i.test(rawTrimmed)) {
          if (!seen.has(rawTrimmed)) {
            seen.add(rawTrimmed);
            items.push({ url: rawTrimmed, label: (label || "Magnet Torrent").trim().slice(0, 80) });
          }
          return;
        }
        try {
          const abs = new URL(rawTrimmed, document.baseURI).href;
          if ((abs.startsWith("http://") || abs.startsWith("https://") || abs.startsWith("ftp://")) && !seen.has(abs)) {
            seen.add(abs);
            items.push({ url: abs, label: (label || "").trim().slice(0, 80) });
          }
        } catch (_) {}
      };
      document.querySelectorAll("a[href]").forEach((el) => addUrl(el.getAttribute("href"), el.textContent));
      document.querySelectorAll("video[src], audio[src], source[src]").forEach((el) => addUrl(el.getAttribute("src"), "Media"));
      return items.slice(0, 500);
    };

    let results = [];
    try {
      results = await chrome.scripting.executeScript({
        target: { tabId: tab.id, allFrames: true },
        func: collectDomLinks
      });
    } catch (_) {
      results = await chrome.scripting.executeScript({
        target: { tabId: tab.id },
        func: collectDomLinks
      });
    }

    const pageLinks = Array.isArray(results)
      ? results.flatMap((r) => (r && Array.isArray(r.result)) ? r.result : [])
      : [];
    const sniffed = tabMediaStreams.get(tab.id) || [];
    const merged = [];
    const seen = new Set();
    for (const s of sniffed) {
      if (!seen.has(s.url)) {
        seen.add(s.url);
        merged.push({ url: s.url, label: s.label || "Stream Media" });
      }
    }
    for (const l of pageLinks) {
      if (!seen.has(l.url)) {
        seen.add(l.url);
        merged.push(l);
      }
    }

    if (merged.length === 0) {
      return { ok: false, error: getBgDict().noLinksFound };
    }

    const pageUrl = tab.url || "";
    const cookieHeader = await collectCookiesHeader(pageUrl, pageUrl);
    const ua = (typeof navigator !== "undefined" && navigator.userAgent) ? navigator.userAgent : "";
    const res = await fetch("http://127.0.0.1:18291/add_links", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        links: merged,
        pageUrl,
        cookie: cookieHeader,
        userAgent: ua
      })
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const data = await res.json().catch(() => ({ count: merged.length }));
    return { ok: true, count: data.count || merged.length };
  } catch (err) {
    return { ok: false, error: err && err.message ? err.message : String(err) };
  }
}

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === "download-cepat-link") {
    const targetUrl = info.linkUrl || info.srcUrl || info.pageUrl;
    if (targetUrl) {
      sendToDownloadCepat(targetUrl, "", tab ? tab.url : "", null, null);
    }
  } else if (info.menuItemId === "download-cepat-all-links" && tab) {
    void collectAndSendAllPageLinks(tab);
  }
});

// Listen to messages from content script (Floating Video Button) or popup
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.action === "SET_LANG" && BG_I18N[message.lang]) {
    currentExtLang = message.lang;
    syncContextMenusLanguage();
    sendResponse({ ok: true });
    return false;
  }

  if (message.action === "GET_TAB_STREAMS") {
    const tabId = typeof message.tabId === "number" ? message.tabId : (sender.tab && sender.tab.id);
    sendResponse({
      ok: true,
      streams: tabId >= 0 ? (tabMediaStreams.get(tabId) || []) : [],
      tabTitle: (sender.tab && sender.tab.title) || ""
    });
    return false;
  }

  if (message.action === "COLLECT_PAGE_LINKS") {
    (async () => {
      let targetTab = sender.tab;
      if (!targetTab && typeof message.tabId === "number") {
        targetTab = await chrome.tabs.get(message.tabId).catch(() => null);
      }
      const result = await collectAndSendAllPageLinks(targetTab);
      sendResponse(result);
    })();
    return true;
  }
  if (message.action === "DOWNLOAD_MEDIA" || message.action === "ADD_DOWNLOAD") {
    sendToDownloadCepat(
      message.url,
      message.pageTitle || message.fileName || "",
      sender.tab ? sender.tab.url : "",
      null,
      null
    );
    sendResponse({ status: "ok" });
    return false;
  }

  if (message.action === "INSPECT_VIDEO") {
    (async () => {
      try {
        const targetUrl = message.url || "";
        const tabUrl = (sender.tab && sender.tab.url) || "";
        const frameUrl = sender.url || "";
        const ref = (tabUrl.startsWith("http://") || tabUrl.startsWith("https://"))
          ? tabUrl
          : ((frameUrl.startsWith("http://") || frameUrl.startsWith("https://")) ? frameUrl : targetUrl);
        const cookieHeader = await collectCookiesHeader(targetUrl, ref, [frameUrl, tabUrl]);
        const ua = (typeof navigator !== "undefined" && navigator.userAgent) ? navigator.userAgent : "";
        const requestBody = {
          url: targetUrl,
          cookie: cookieHeader,
          referer: ref,
          referrer: ref,
          userAgent: ua
        };

        try {
          const controller = new AbortController();
          const timeoutId = setTimeout(() => controller.abort(), 60000);
          const res = await fetch("http://127.0.0.1:18291/inspect", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(requestBody),
            signal: controller.signal
          });
          clearTimeout(timeoutId);
          if (!res.ok) {
            throw new Error(`HTTP error ${res.status}`);
          }
          const data = await res.json();
          sendResponse({ ok: true, data });
          return;
        } catch (_httpErr) {
          const nativeData = await sendNativeHostRequest({
            action: "INSPECT_VIDEO",
            ...requestBody
          });
          if (nativeData && nativeData.status === "ok") {
            sendResponse({ ok: true, data: nativeData });
          } else {
            throw new Error((nativeData && nativeData.message) || getBgDict().desktopNoResp);
          }
        }
      } catch (err) {
        sendResponse({ ok: false, error: err && err.message ? err.message : String(err) });
      }
    })();
    return true;
  }

  if (message.action === "START_VIDEO_DOWNLOAD") {
    (async () => {
      try {
        const payload = Object.assign({}, message.payload || {});
        const targetUrl = payload.url || "";
        const tabUrl = (sender.tab && sender.tab.url) || "";
        const frameUrl = sender.url || "";
        const defaultRef = (tabUrl.startsWith("http://") || tabUrl.startsWith("https://"))
          ? tabUrl
          : ((frameUrl.startsWith("http://") || frameUrl.startsWith("https://")) ? frameUrl : targetUrl);
        const effectiveRef = payload.referer || payload.referrer || defaultRef;
        const cookieHeader = await collectCookiesHeader(targetUrl, effectiveRef, [frameUrl, tabUrl]);
        const ua = (typeof navigator !== "undefined" && navigator.userAgent) ? navigator.userAgent : "";
        payload.cookie = payload.cookie || cookieHeader;
        payload.referer = effectiveRef;
        payload.referrer = payload.referrer || effectiveRef;
        payload.userAgent = payload.userAgent || ua;

        try {
          const controller = new AbortController();
          const timeoutId = setTimeout(() => controller.abort(), 10000);
          const res = await fetch("http://127.0.0.1:18291/start_download", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(payload),
            signal: controller.signal
          });
          clearTimeout(timeoutId);
          if (!res.ok) {
            throw new Error(`HTTP error ${res.status}`);
          }
          const data = await res.json().catch(() => ({ status: "ok" }));
          sendResponse({ ok: true, data });
          return;
        } catch (_httpErr) {
          const nativeData = await sendNativeHostRequest({
            action: "START_VIDEO_DOWNLOAD",
            ...payload
          });
          if (nativeData && nativeData.status === "ok") {
            sendResponse({ ok: true, data: nativeData });
          } else {
            throw new Error((nativeData && nativeData.message) || getBgDict().desktopNoResp);
          }
        }
      } catch (err) {
        sendResponse({ ok: false, error: err && err.message ? err.message : String(err) });
      }
    })();
    return true;
  }

  return false;
});

// Native Messaging Host IPC helper
async function sendNativeHostRequest(payload) {
  return new Promise((resolve, reject) => {
    try {
      chrome.runtime.sendNativeMessage(NATIVE_HOST, payload, (response) => {
        if (chrome.runtime.lastError) {
          reject(new Error(chrome.runtime.lastError.message));
          return;
        }
        resolve(response || {});
      });
    } catch (err) {
      reject(err);
    }
  });
}

// Communicate with Download Cepat Desktop Engine
async function sendToDownloadCepat(url, filename, referrer, fileSize = null, mimeType = null) {
  const effectiveReferrer = await resolveEffectiveReferrer(referrer);
  const cookieHeader = await collectCookiesHeader(url, effectiveReferrer);
  const userAgent = (typeof navigator !== "undefined" && navigator.userAgent) ? navigator.userAgent : "";

  const payload = {
    url: url,
    fileName: filename || "",
    referrer: effectiveReferrer,
    referer: effectiveReferrer,
    cookie: cookieHeader,
    userAgent: userAgent,
    fileSize: (typeof fileSize === "number" && fileSize > 0) ? fileSize : null,
    mimeType: mimeType || null
  };

  // 1. Try local HTTP bridge (port 18291)
  try {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 1800);
    const res = await fetch("http://127.0.0.1:18291/add_download", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
      signal: controller.signal
    });
    clearTimeout(timeoutId);
    if (res.ok) {
      console.log("[Download Cepat] Berhasil dikirim via local bridge HTTP (cookie:", Boolean(cookieHeader), ")");
      return true;
    }
  } catch (_e) {
    // Local bridge server not reached, try fallback
  }

  // 2. Fallback to Native Messaging Host IPC (auto-launches desktop app if closed)
  try {
    const response = await sendNativeHostRequest({
      action: "ADD_DOWNLOAD",
      ...payload,
      timestamp: Date.now()
    });
    console.log("[Download Cepat] Respons desktop:", response);
    return true;
  } catch (err) {
    console.warn("[Download Cepat] Native messaging host belum aktif:", err && err.message ? err.message : err);
    return false;
  }
}
