import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';

const ROOT_DIR = path.resolve(__dirname, '../..');
const EXT_DIR = path.join(ROOT_DIR, 'extension');

function loadBackgroundHarness(options?: {
  fetchImpl?: (url: string, init?: RequestInit) => Promise<any>;
  executeScriptImpl?: (injection: any) => Promise<any>;
  cookiesByUrl?: Record<string, Array<{ name: string; value: string }>>;
}) {
  const code = fs.readFileSync(path.join(EXT_DIR, 'background.js'), 'utf8');

  let webRequestListener: ((details: any) => void) | null = null;
  let downloadsListener: ((item: any, suggest: () => void) => boolean) | null = null;
  let onMessageListener: ((msg: any, sender: any, sendResponse: (r: any) => void) => boolean | void) | null = null;
  let tabRemovedListener: ((tabId: number) => void) | null = null;

  const fetchMock = vi.fn(
    options?.fetchImpl ||
      (async () => ({
        ok: true,
        status: 200,
        json: async () => ({ status: 'ok' }),
      }))
  );

  const executeScriptMock = vi.fn(
    options?.executeScriptImpl ||
      (async () => [{ result: [{ url: 'https://parent.example.com/a.zip', label: 'A' }] }])
  );

  const chromeMock: any = {
    storage: {
      local: {
        get: vi.fn((_keys: any, cb: (res: any) => void) => cb({})),
        set: vi.fn(),
      },
      onChanged: { addListener: vi.fn() },
    },
    contextMenus: {
      update: vi.fn((_id: string, _props: any, cb?: () => void) => cb && cb()),
      removeAll: vi.fn((cb?: () => void) => cb && cb()),
      create: vi.fn(),
      onClicked: { addListener: vi.fn() },
    },
    webRequest: {
      onResponseStarted: {
        addListener: vi.fn((fn: (d: any) => void) => {
          webRequestListener = fn;
        }),
      },
    },
    tabs: {
      onRemoved: {
        addListener: vi.fn((fn: (id: number) => void) => {
          tabRemovedListener = fn;
        }),
      },
      onUpdated: { addListener: vi.fn() },
      query: vi.fn(async () => []),
      get: vi.fn(async (id: number) => ({ id, url: 'https://parent.example.com/watch', title: 'Watch Page' })),
    },
    downloads: {
      onDeterminingFilename: {
        addListener: vi.fn((fn: (item: any, suggest: () => void) => boolean) => {
          downloadsListener = fn;
        }),
      },
      cancel: vi.fn((_id: number, cb?: () => void) => cb && cb()),
      erase: vi.fn(),
    },
    cookies: {
      getAll: vi.fn(async ({ url }: { url: string }) => {
        if (!options?.cookiesByUrl) return [];
        return options.cookiesByUrl[url] || [];
      }),
    },
    scripting: {
      executeScript: executeScriptMock,
    },
    runtime: {
      lastError: null,
      onInstalled: { addListener: vi.fn() },
      onMessage: {
        addListener: vi.fn((fn: any) => {
          onMessageListener = fn;
        }),
      },
      sendNativeMessage: vi.fn((_host: string, _payload: any, cb: (resp: any) => void) => cb({ status: 'ok' })),
    },
  };

  const sandbox: any = {
    chrome: chromeMock,
    fetch: fetchMock,
    URL,
    AbortController,
    setTimeout,
    clearTimeout,
    Map,
    Set,
    Array,
    Object,
    Promise,
    Error,
    navigator: { userAgent: 'MockChrome/130' },
    console,
  };
  vm.createContext(sandbox);
  vm.runInContext(code, sandbox);

  const sendMessage = (msg: any, sender: any = {}): Promise<any> =>
    new Promise((resolve) => {
      if (!onMessageListener) throw new Error('onMessage listener missing');
      let settled = false;
      const done = (resp: any) => {
        if (!settled) {
          settled = true;
          resolve(resp);
        }
      };
      const isAsync = onMessageListener(msg, sender, done);
      if (!isAsync && !settled) {
        done(undefined);
      }
    });

  return {
    sandbox,
    chromeMock,
    fetchMock,
    executeScriptMock,
    triggerWebRequest: (details: any) => webRequestListener && webRequestListener(details),
    triggerDeterminingFilename: (item: any, suggest = vi.fn()) =>
      downloadsListener ? downloadsListener(item, suggest) : false,
    triggerTabRemoved: (tabId: number) => tabRemovedListener && tabRemovedListener(tabId),
    sendMessage,
  };
}

describe('Extension Manifest, Iframe Support & .ts Detection', () => {
  beforeEach(() => {
    document.body.innerHTML = '';
    document.head.innerHTML = '';
    document.title = '';
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('manifest.json enables all_frames for iframe detection and meets Chrome Web Store description limit', () => {
    const manifest = JSON.parse(fs.readFileSync(path.join(EXT_DIR, 'manifest.json'), 'utf8'));
    expect(manifest.content_scripts).toHaveLength(1);
    expect(manifest.content_scripts[0].matches).toEqual(['<all_urls>']);
    expect(manifest.content_scripts[0].all_frames).toBe(true);
    expect(manifest.description.length).toBeLessThanOrEqual(132);
    expect(manifest.host_permissions).toContain('<all_urls>');
  });

  it('background sniffer skips tiny HLS .ts fragments and TypeScript files while capturing full .ts videos', async () => {
    const h = loadBackgroundHarness();
    const tabId = 42;

    // 1. HLS .ts fragment (< 5 MB) must be skipped
    h.triggerWebRequest({
      tabId,
      type: 'xmlhttprequest',
      url: 'https://cdn.example.com/hls/seg-001.ts?token=abc',
      responseHeaders: [
        { name: 'Content-Type', value: 'video/mp2t' },
        { name: 'Content-Length', value: String(650 * 1024) },
      ],
    });

    // 2. TypeScript .ts script must be skipped even without Content-Length
    h.triggerWebRequest({
      tabId,
      type: 'script',
      url: 'https://app.example.com/src/main.ts',
      responseHeaders: [{ name: 'Content-Type', value: 'application/typescript' }],
    });
    h.triggerWebRequest({
      tabId,
      type: 'xmlhttprequest',
      url: 'https://app.example.com/raw/types.ts',
      responseHeaders: [{ name: 'Content-Type', value: 'text/plain; charset=utf-8' }],
    });

    // 3. Large standalone .ts video with application/octet-stream (>= 5 MB) must be detected
    h.triggerWebRequest({
      tabId,
      type: 'xmlhttprequest',
      url: 'https://cdn.example.com/vod/full_movie.ts?dl=1',
      responseHeaders: [
        { name: 'Content-Type', value: 'application/octet-stream' },
        { name: 'Content-Length', value: String(25 * 1024 * 1024) },
      ],
    });

    // 4. Large .ts video with no Content-Type header (>= 5 MB) must also be detected
    h.triggerWebRequest({
      tabId,
      type: 'media',
      url: 'https://cdn.example.com/vod/capture_2.ts',
      responseHeaders: [{ name: 'Content-Length', value: String(8 * 1024 * 1024) }],
    });

    const resp = await h.sendMessage({ action: 'GET_TAB_STREAMS', tabId }, { tab: { id: tabId, title: 'My Video' } });
    expect(resp.ok).toBe(true);
    expect(resp.tabTitle).toBe('My Video');
    expect(resp.streams).toHaveLength(2);

    const urls = resp.streams.map((s: any) => s.url);
    expect(urls).toContain('https://cdn.example.com/vod/full_movie.ts?dl=1');
    expect(urls).toContain('https://cdn.example.com/vod/capture_2.ts');
    expect(urls).not.toContain('https://cdn.example.com/hls/seg-001.ts?token=abc');
    expect(urls).not.toContain('https://app.example.com/src/main.ts');
    expect(resp.streams.every((s: any) => s.mimeType === 'video/mp2t')).toBe(true);
    expect(resp.streams.every((s: any) => s.label.startsWith('TS Video ('))).toBe(true);
  });

  it('background sniffer prevents chunked HLS .ts segments without Content-Length from flooding an .m3u8 tab', async () => {
    const h = loadBackgroundHarness();
    const tabId = 77;

    // Standalone chunked MPEG-TS (no .m3u8 on tab yet) is captured
    h.triggerWebRequest({
      tabId: 78,
      type: 'media',
      url: 'https://live.example.com/channel.ts',
      responseHeaders: [{ name: 'Content-Type', value: 'video/mp2t' }],
    });
    const standaloneResp = await h.sendMessage({ action: 'GET_TAB_STREAMS', tabId: 78 });
    expect(standaloneResp.streams.map((s: any) => s.url)).toContain('https://live.example.com/channel.ts');

    // Tab 77 already has an .m3u8 manifest; chunked .ts segments without Content-Length are ignored
    h.triggerWebRequest({
      tabId,
      type: 'xmlhttprequest',
      url: 'https://hls.example.com/master.m3u8',
      responseHeaders: [{ name: 'Content-Type', value: 'application/vnd.apple.mpegurl' }],
    });
    h.triggerWebRequest({
      tabId,
      type: 'xmlhttprequest',
      url: 'https://hls.example.com/chunk-1.ts',
      responseHeaders: [{ name: 'Content-Type', value: 'video/mp2t' }],
    });

    const hlsResp = await h.sendMessage({ action: 'GET_TAB_STREAMS', tabId });
    expect(hlsResp.streams).toHaveLength(1);
    expect(hlsResp.streams[0].url).toBe('https://hls.example.com/master.m3u8');
  });

  it('onDeterminingFilename intercepts application/mp2t and large .ts downloads while ignoring TypeScript .ts files', () => {
    const h = loadBackgroundHarness();

    // Small TypeScript file -> NOT intercepted
    expect(
      h.triggerDeterminingFilename({
        id: 1,
        url: 'https://raw.example.com/index.ts',
        filename: 'index.ts',
        mime: 'text/plain',
        fileSize: 4096,
      })
    ).toBe(false);

    // Direct application/mp2t download -> intercepted
    expect(
      h.triggerDeterminingFilename({
        id: 2,
        url: 'https://cdn.example.com/stream.ts',
        filename: 'stream.ts',
        mime: 'application/mp2t',
        fileSize: 2 * 1024 * 1024,
      })
    ).toBe(true);

    // Large >= 5 MB .ts download with missing MIME -> intercepted
    expect(
      h.triggerDeterminingFilename({
        id: 3,
        url: 'https://cdn.example.com/movie.ts',
        filename: 'movie.ts',
        mime: '',
        fileSize: 10 * 1024 * 1024,
      })
    ).toBe(true);
  });

  it('merges frame + parent cookies/referer for INSPECT_VIDEO & START_VIDEO_DOWNLOAD from cross-origin iframes', async () => {
    const h = loadBackgroundHarness({
      cookiesByUrl: {
        'https://cdn.video.net/movie.ts': [{ name: 'cdn_tok', value: '111' }],
        'https://player.embed.tv/v/99': [{ name: 'player_sess', value: '222' }],
        'https://parent.site.org/article': [{ name: 'site_auth', value: '333' }],
      },
    });

    await h.sendMessage(
      { action: 'INSPECT_VIDEO', url: 'https://player.embed.tv/v/99' },
      { url: 'https://player.embed.tv/v/99', tab: { id: 5, url: 'https://parent.site.org/article' } }
    );

    const inspectCall = h.fetchMock.mock.calls.find(([u]) => String(u).endsWith('/inspect'));
    expect(inspectCall).toBeDefined();
    const inspectBody = JSON.parse((inspectCall![1] as RequestInit).body as string);
    expect(inspectBody.referer).toBe('https://parent.site.org/article');
    expect(inspectBody.cookie).toContain('player_sess=222');
    expect(inspectBody.cookie).toContain('site_auth=333');

    await h.sendMessage(
      {
        action: 'START_VIDEO_DOWNLOAD',
        payload: {
          url: 'https://cdn.video.net/movie.ts',
          title: 'Embedded Movie',
          formatId: '',
          extension: 'ts',
          referer: 'https://player.embed.tv/v/99',
        },
      },
      { url: 'https://player.embed.tv/v/99', tab: { id: 5, url: 'https://parent.site.org/article' } }
    );

    const dlCall = h.fetchMock.mock.calls.find(([u]) => String(u).endsWith('/start_download'));
    expect(dlCall).toBeDefined();
    const dlBody = JSON.parse((dlCall![1] as RequestInit).body as string);
    expect(dlBody.referer).toBe('https://player.embed.tv/v/99');
    expect(dlBody.cookie).toContain('cdn_tok=111');
    expect(dlBody.cookie).toContain('player_sess=222');
    expect(dlBody.cookie).toContain('site_auth=333');
  });

  it('collectAndSendAllPageLinks queries allFrames with fallback to main frame', async () => {
    const h = loadBackgroundHarness({
      executeScriptImpl: vi
        .fn()
        .mockResolvedValueOnce([
          { frameId: 0, result: [{ url: 'https://site.com/file.zip', label: 'Zip' }] },
          { frameId: 2, result: [{ url: 'https://iframe.cdn/embed.mp4', label: 'Media' }] },
        ])
        .mockRejectedValueOnce(new Error('frame denied'))
        .mockResolvedValueOnce([
          { frameId: 0, result: [{ url: 'https://site.com/only-main.zip', label: 'Main' }] },
        ]),
    });

    const res = await h.sendMessage({ action: 'COLLECT_PAGE_LINKS', tabId: 9 });
    expect(res.ok).toBe(true);
    expect(res.count).toBe(2);
    expect(h.executeScriptMock).toHaveBeenCalledWith(
      expect.objectContaining({ target: expect.objectContaining({ tabId: 9, allFrames: true }) })
    );

    const fallbackRes = await h.sendMessage({ action: 'COLLECT_PAGE_LINKS', tabId: 10 });
    expect(fallbackRes.ok).toBe(true);
    expect(fallbackRes.count).toBe(1);
    expect(h.executeScriptMock).toHaveBeenLastCalledWith(
      expect.objectContaining({ target: { tabId: 10 } })
    );
  });

  it('content.js skips tiny preview <video>, hooks normal/resized <video>, supports .ts badges & fallback on HTTP and about:blank frames', async () => {
    const { JSDOM } = await import('jsdom');
    const contentCode = fs.readFileSync(path.join(EXT_DIR, 'content.js'), 'utf8');

    const sentMessages: any[] = [];
    (globalThis as any).chrome = {
      storage: {
        local: { get: (_k: any, cb: (r: any) => void) => cb({}) },
        onChanged: { addListener: () => {} },
      },
      runtime: {
        lastError: null,
        sendMessage: (msg: any, cb: (r: any) => void) => {
          sentMessages.push(msg);
          if (msg.action === 'INSPECT_VIDEO') {
            cb({ ok: true, data: { status: 'error', message: 'unsupported site' } });
            return;
          }
          if (msg.action === 'GET_TAB_STREAMS') {
            cb({
              ok: true,
              tabTitle: 'Embedded Drama Ep 1',
              streams: [
                {
                  url: 'https://cdn.example.com/drama/ep1.ts?sig=xyz',
                  mimeType: 'video/mp2t',
                  fileSize: 40 * 1024 * 1024,
                  label: 'TS Video (ep1.ts)',
                },
                {
                  url: 'https://cdn.example.com/drama/seg-1.ts',
                  mimeType: 'video/mp2t',
                  fileSize: 512 * 1024,
                  label: 'TS Video (seg-1.ts)',
                },
              ],
            });
            return;
          }
          if (msg.action === 'START_VIDEO_DOWNLOAD') {
            cb({ ok: true, data: { status: 'ok' } });
            return;
          }
          cb({ ok: false, error: 'unexpected' });
        },
      },
    };

    try {
      // Part A: HTTP page/frame with resize from tiny preview to real player + fallback to GET_TAB_STREAMS
      const wrapper = document.createElement('div');
      const video = document.createElement('video');
      let mockRect = { width: 80, height: 45, top: 0, left: 0, right: 80, bottom: 45, x: 0, y: 0, toJSON: () => ({}) };
      (video as any).getBoundingClientRect = () => mockRect;
      wrapper.appendChild(video);
      document.body.appendChild(wrapper);

      const runContent = new Function(
        `${contentCode}\nreturn { initVideoSniffer, resolveMediaExtension };`
      );
      const api = runContent();

      expect(wrapper.querySelector('.download-cepat-video-container')).toBeNull();
      expect(api.resolveMediaExtension('https://cdn.example.com/drama/ep1.ts?sig=xyz', '')).toBe('ts');
      expect(api.resolveMediaExtension('https://cdn.example.com/stream', 'video/mp2t')).toBe('ts');

      mockRect = { width: 640, height: 360, top: 0, left: 0, right: 640, bottom: 360, x: 0, y: 0, toJSON: () => ({}) };
      video.dispatchEvent(new Event('loadedmetadata'));
      const btn = wrapper.querySelector('.download-cepat-btn') as HTMLElement;
      const dropdown = wrapper.querySelector('.download-cepat-dropdown') as HTMLElement;
      expect(btn).not.toBeNull();
      expect(dropdown.style.maxWidth).toContain('100vw');
      expect(dropdown.style.maxHeight).toContain('420px');

      btn.click();
      await new Promise((r) => setTimeout(r, 20));

      const formatItems = dropdown.querySelectorAll('.dc-format-item');
      expect(formatItems.length).toBe(1);
      expect(dropdown.textContent).toContain('TS');
      expect(dropdown.textContent).toContain('TS Video (ep1.ts)');
      expect(dropdown.textContent).not.toContain('belum terhubung');

      (formatItems[0] as HTMLElement).click();
      await new Promise((r) => setTimeout(r, 20));

      const startMsg = sentMessages.find((m) => m.action === 'START_VIDEO_DOWNLOAD');
      expect(startMsg).toBeDefined();
      expect(startMsg.payload).toMatchObject({
        url: 'https://cdn.example.com/drama/ep1.ts?sig=xyz',
        title: 'Embedded Drama Ep 1',
        extension: 'ts',
        formatId: '',
      });

      // Part B: Genuine about:blank frame skips INSPECT_VIDEO and uses GET_TAB_STREAMS without disconnected error
      sentMessages.length = 0;
      const blankDom = new JSDOM('<!doctype html><html><body><div><video></video></div></body></html>', {
        url: 'about:blank',
        runScripts: 'outside-only',
      });
      blankDom.window.chrome = (globalThis as any).chrome;
      blankDom.window.eval(contentCode);
      const blankBtn = blankDom.window.document.querySelector('.download-cepat-btn') as HTMLElement;
      const blankDropdown = blankDom.window.document.querySelector('.download-cepat-dropdown') as HTMLElement;
      expect(blankBtn).not.toBeNull();

      blankBtn.click();
      await new Promise((r) => setTimeout(r, 20));

      expect(sentMessages.some((m) => m.action === 'INSPECT_VIDEO')).toBe(false);
      expect(sentMessages.some((m) => m.action === 'GET_TAB_STREAMS')).toBe(true);
      expect(blankDropdown.textContent).toContain('TS Video (ep1.ts)');
      expect(blankDropdown.textContent).not.toContain('belum terhubung');
      blankDom.window.close();
    } finally {
      delete (globalThis as any).chrome;
    }
  });
});
