# Privacy Policy — Download Cepat Integration Module

**Effective Date:** November 12, 2026  
**Developer:** Ujang Mustamar  
**Contact:** moestamar.nawawie@gmail.com  
**Repository:** https://github.com/moesstudio/download-cepat

---

## 1. Overview & Single Purpose

**Download Cepat Integration Module** is a companion browser extension whose single purpose is to connect Google Chrome (and Chromium-based browsers) to the local **Download Cepat** desktop application running on the user's own computer so downloads, video/audio stream selections, and page links can be accelerated natively.

We respect user privacy strictly. This extension **does not collect, store on remote servers, sell, or share any personal or browsing data with the developer or any third party.**

---

## 2. Information Handled Locally On-Device

To perform its download-management functions, the extension reads limited information strictly on your local device:

| Data Type | When & Why It Is Read | Where It Goes |
| :--- | :--- | :--- |
| **Download URLs, filenames, MIME types, and file sizes** | When you click a downloadable file intercepted via `chrome.downloads.onDeterminingFilename`, select a media stream format, or choose a Download Cepat context menu action. | Sent locally only to your desktop Download Cepat app (`http://127.0.0.1:18291` or Chrome Native Messaging Host `com.downloadcepat.nativehost`). |
| **Detected media stream URLs** | Passively observed from `chrome.webRequest.onResponseStarted` headers on the active tab so the video overlay can list downloadable streams (e.g., `.mp4`, `.m3u8`, `.mpd`). Cleared when the tab reloads or closes. | Stored temporarily in local extension memory per tab and sent to your local desktop app only when you click download. |
| **Active page URL (`Referer`) & domain session cookies** | Read via `chrome.tabs` and `chrome.cookies` **only** for the target download domain/referrer when handing off a download task, allowing authenticated/session-protected downloads to succeed natively. | Transmitted strictly over localhost (`127.0.0.1`) or Chrome Native Messaging on your machine to your desktop app's download request headers. |
| **Extension preferences (`dcEnabled`, `dcExcludedDomains`, `dcLang`)** | Stored in `chrome.storage.local` to remember whether interception is enabled, which hostnames are excluded, and your chosen display language (`id`, `en`, `ms`, `ja`). | Stays locally inside browser storage on your device. |

---

## 3. What We Never Do

- **No Remote Servers or Cloud Telemetry:** The extension makes **zero** network connections to external analytics, tracking, ad networks, or developer servers.
- **No Data Selling or Sharing:** We never sell, rent, trade, or transfer user data to third parties or data brokers.
- **No Unrelated Background Scraping:** Page link extraction (`chrome.scripting`) executes only on the active tab when you explicitly click *"Download All Links on Page"*.

---

## 4. Chrome Web Store User Data Policy (Limited Use)

The use of information received from Chrome APIs strictly adheres to the **Chrome Web Store User Data Policy**, including the **Limited Use** requirements. Information accessed by the extension is used solely to provide and execute user-initiated download handoff and media capture features locally on the user's device.

---

## 5. Contact & Changes

If you have questions about this privacy policy or wish to inspect the open-source code, please visit https://github.com/moesstudio/download-cepat or contact **Ujang Mustamar** at `moestamar.nawawie@gmail.com`.

---

## Ringkasan Bahasa Indonesia

Ekstensi **Download Cepat Integration Module** bekerja sepenuhnya secara lokal di perangkat Anda untuk meneruskan tautan unduhan, pilihan resolusi video/audio, daftar tautan halaman (atas klik pengguna), dan cookie sesi domain target ke aplikasi desktop **Download Cepat** melalui koneksi lokal (`127.0.0.1` / Native Messaging). Ekstensi ini **tidak mengirim data apa pun ke server eksternal, tidak melacak riwayat penelusuran, tidak menampilkan iklan, dan tidak menjual atau membagikan data pengguna ke pihak ketiga**.
