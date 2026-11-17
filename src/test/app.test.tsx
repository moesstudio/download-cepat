import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import App from '../App.tsx';
import { AddUrlModal } from '../components/AddUrlModal.tsx';
import { SegmentedProgressBar } from '../components/SegmentedProgressBar.tsx';
import { DownloadProgressModal } from '../components/DownloadProgressModal.tsx';
import { PopupDownloadWindow } from '../components/PopupDownloadWindow.tsx';
import { I18nProvider } from '../i18n/index.tsx';
import * as nativeBridge from '../services/nativeBridge.ts';
import {
  expandBatchPattern,
  parseUrlListText,
  formatProxyUrl,
  findMatchingSiteLogin,
  getParentDir,
  joinDestinationPath,
  matchCategoryForFile,
  isWithinScheduleWindow,
  shouldInterceptClipboardUrl,
  isMagnetUri,
  isTorrentReference,
  DEFAULT_CATEGORY_RULES,
  DEFAULT_CLIPBOARD_CONFIG,
  DEFAULT_SCHEDULER_CONFIG,
} from '../utils/downloadUtils.ts';

describe('Download Cepat UI Suite', () => {
  beforeEach(() => {
    localStorage.clear();
    if (typeof document !== 'undefined') {
      document.documentElement.lang = 'en';
      document.documentElement.dir = 'ltr';
    }
  });

  it('F01-H1: Renders main dashboard in English default with seeded tasks and category counts', () => {
    render(<App />);

    // Brand and headers
    expect(screen.getByText('Download Cepat')).toBeInTheDocument();
    expect(screen.getByText('High-Speed Engine v1.0')).toBeInTheDocument();
    expect(document.documentElement.lang).toBe('en');
    expect(document.documentElement.dir).toBe('ltr');

    // Default tasks rendered
    expect(screen.getByText('download-cepat-main.zip')).toBeInTheDocument();
    expect(screen.getByText('Alacritty-v0.17.0.dmg')).toBeInTheDocument();
    expect(screen.getByText('RFC_9110_HTTP_Semantics.pdf')).toBeInTheDocument();
    expect(screen.getByText('Yura Yunita - Dunia Tipu-Tipu.mp4')).toBeInTheDocument();
    expect(screen.getByText('Yura Yunita - Dunia Tipu-Tipu.mp3')).toBeInTheDocument();
    expect(screen.getByText('Torrents')).toBeInTheDocument();
  });

  it('F01-H2: Opens Add URL modal and adds a new download task', () => {
    render(<App />);

    // Click "Add URL"
    const addBtn = screen.getByRole('button', { name: /Add URL/i });
    fireEvent.click(addBtn);

    // Modal is opened
    expect(screen.getByText('Add New Download')).toBeInTheDocument();

    // Fill URL
    const urlInput = screen.getByPlaceholderText(/https:\/\/example\.com\/file\.zip/i);
    fireEvent.change(urlInput, { target: { value: 'https://example.com/new_dataset.zip' } });

    // Click "Start Download"
    const submitBtn = screen.getByRole('button', { name: /Start Download/i });
    fireEvent.click(submitBtn);

    // Modal closed and new task is in table and active progress modal
    expect(screen.queryByText('Add New Download')).not.toBeInTheDocument();
    expect(screen.getAllByText('new_dataset.zip').length).toBeGreaterThan(0);
  });

  it('F01-E1: Empty URL disables download button', () => {
    render(<App />);

    const addBtn = screen.getByRole('button', { name: /Add URL/i });
    fireEvent.click(addBtn);

    const submitBtn = screen.getByRole('button', { name: /Start Download/i });
    expect(submitBtn).toBeDisabled();
  });

  it('F02-H1: Filters task table when searching by filename', () => {
    render(<App />);

    const searchInput = screen.getByPlaceholderText('Search files...');
    fireEvent.change(searchInput, { target: { value: 'RFC' } });

    expect(screen.getByText('RFC_9110_HTTP_Semantics.pdf')).toBeInTheDocument();
    expect(screen.queryByText('Alacritty-v0.17.0.dmg')).not.toBeInTheDocument();

    // Clear search
    fireEvent.change(searchInput, { target: { value: '' } });
    expect(screen.getByText('Alacritty-v0.17.0.dmg')).toBeInTheDocument();
  });

  it('F02-E1: Category filter switching', () => {
    render(<App />);

    // Click "Music" filter in sidebar
    const musicTab = screen.getByRole('button', { name: /Music/i });
    fireEvent.click(musicTab);

    // Music tasks shown
    expect(screen.getByText('Yura Yunita - Dunia Tipu-Tipu.mp3')).toBeInTheDocument();
    // Programs task should not be shown
    expect(screen.queryByText('Alacritty-v0.17.0.dmg')).not.toBeInTheDocument();
  });

  it('F03-H1: Pause and resume download controls', () => {
    render(<App />);

    // Select active task row
    const taskRow = screen.getByText('download-cepat-main.zip');
    fireEvent.click(taskRow);

    // Click "Pause"
    const pauseBtn = screen.getByRole('button', { name: /Pause/i });
    fireEvent.click(pauseBtn);

    // Status changes to Paused
    expect(screen.getAllByText('Paused').length).toBeGreaterThan(0);

    // Click "Resume"
    const resumeBtn = screen.getByRole('button', { name: /Resume/i });
    fireEvent.click(resumeBtn);

    expect(screen.getAllByText('Downloading').length).toBeGreaterThan(0);
  });

  it('F04-H1: Speed Limiter modal allows toggling and setting custom bandwidth limit', () => {
    render(<App />);

    const settingsBtn = screen.getByTitle('Settings & Speed Limiter');
    fireEvent.click(settingsBtn);

    expect(screen.getByText('Settings & Speed Limiter')).toBeInTheDocument();

    // Check speed limiter checkbox
    const checkbox = screen.getByLabelText(/Enable Speed Limiter/i);
    fireEvent.click(checkbox);

    // Save
    const saveBtn = screen.getByRole('button', { name: /Save Settings/i });
    fireEvent.click(saveBtn);

    expect(screen.queryByText('Settings & Speed Limiter')).not.toBeInTheDocument();
    expect(screen.getByText(/Limit: 2048 KB\/s/i)).toBeInTheDocument();
  });

  it('S04: Renders SegmentedProgressBar in compact and detailed mode', () => {
    const { rerender } = render(
      <SegmentedProgressBar
        progressPercent={45.5}
        status="downloading"
        variant="compact"
      />
    );
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '45.5');

    // Detailed mode with 8 threads
    rerender(
      <SegmentedProgressBar
        progressPercent={70.0}
        status="downloading"
        variant="detailed"
        numConnections={8}
      />
    );
    expect(screen.getByText('70.0%')).toBeInTheDocument();
    expect(screen.getByText(/Parallel Connections \(8 threads\)/i)).toBeInTheDocument();
  });

  it('F01-V1: Pasting video link detects video media and allows resolution and format selection', async () => {
    render(<App />);

    const addBtn = screen.getByRole('button', { name: /Add URL/i });
    fireEvent.click(addBtn);

    const urlInput = screen.getByPlaceholderText(/https:\/\/example\.com\/file\.zip/i);
    fireEvent.change(urlInput, { target: { value: 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' } });

    // Video media detected card should appear after probe
    await screen.findByText('Video Media Detected');

    // Available formats
    expect(screen.getByText('1080p (Full HD 60fps)')).toBeInTheDocument();
    expect(screen.getByText('720p (HD)')).toBeInTheDocument();
    expect(screen.getByText('Audio Only (MP3 320 kbps)')).toBeInTheDocument();

    // Select Audio Only MP3
    const mp3Btn = screen.getByText('Audio Only (MP3 320 kbps)');
    fireEvent.click(mp3Btn);

    // Filename should update to .mp3
    const fileNameInput = screen.getByDisplayValue(/.*\.mp3/i);
    expect(fileNameInput).toBeInTheDocument();

    // Submit download
    const submitBtn = screen.getByRole('button', { name: /Start Download/i });
    fireEvent.click(submitBtn);

    // Download started and appears in list
    expect(screen.getAllByText(/.*\.mp3/i).length).toBeGreaterThan(0);
  });

  it('F05: Multi-select checkboxes mark tasks and toolbar delete marked works', () => {
    render(<App />);

    // Mark the first file checkbox
    const markCheckbox = screen.getByLabelText(/Mark download-cepat-main.zip/i);
    fireEvent.click(markCheckbox);

    // Delete marked button should appear in toolbar
    const deleteMarkedBtn = screen.getByRole('button', { name: /Delete Marked \(1\)/i });
    expect(deleteMarkedBtn).toBeInTheDocument();

    // Click delete marked
    fireEvent.click(deleteMarkedBtn);

    // download-cepat-main.zip should no longer be in the document
    expect(screen.queryByText('download-cepat-main.zip')).not.toBeInTheDocument();
  });

  it('F06: Settings changes threads and max concurrent tasks', () => {
    render(<App />);

    const settingsBtn = screen.getByTitle('Settings & Speed Limiter');
    fireEvent.click(settingsBtn);

    // Change thread count to 16
    const selectInputs = screen.getAllByRole('combobox');
    const threadSelect = selectInputs.find((s) => s.innerHTML.includes('16 Threads'));
    if (threadSelect) {
      fireEvent.change(threadSelect, { target: { value: '16' } });
    }

    // Save
    const saveBtn = screen.getByRole('button', { name: /Save Settings/i });
    fireEvent.click(saveBtn);

    // Sidebar should now display 16 Threads / Task
    expect(screen.getByText('16 Threads / Task')).toBeInTheDocument();
  });

  it('F07: AddUrlModal forwards captured browser session cookies and referer', () => {
    const onStartDownload = vi.fn();
    render(
      <AddUrlModal
        isOpen={true}
        onClose={vi.fn()}
        onStartDownload={onStartDownload}
        categories={[
          { id: 'compressed', name: 'Compressed', icon: 'archive', extensions: ['.7z', '.zip'], count: 0 },
        ]}
        incomingMeta={{
          url: 'https://store-na-phx-3.gofile.io/download/web/507c/W11Pro.7z',
          fileName: 'W11Pro.7z',
          cookie: 'accountToken=abc123',
          referer: 'https://gofile.io/',
          userAgent: 'Mozilla/5.0 Chrome/130.0.0.0',
        }}
        defaultConnections={16}
      />
    );

    expect(screen.getByText('Active Browser Session Attached')).toBeInTheDocument();
    expect(screen.getByDisplayValue('W11Pro.7z')).toBeInTheDocument();

    const submitBtn = screen.getByRole('button', { name: /Start Download/i });
    fireEvent.click(submitBtn);

    expect(onStartDownload).toHaveBeenCalledTimes(1);
    const callArgs = onStartDownload.mock.calls[0];
    expect(callArgs[0]).toBe('https://store-na-phx-3.gofile.io/download/web/507c/W11Pro.7z');
    expect(callArgs[1]).toBe('W11Pro.7z');
    expect(callArgs[4]).toBe(16);
    expect(callArgs[7]).toMatchObject({
      cookie: 'accountToken=abc123',
      referer: 'https://gofile.io/',
      userAgent: 'Mozilla/5.0 Chrome/130.0.0.0',
    });
  });

  it('F08: SegmentedProgressBar renders real per-thread widths and DownloadProgressModal renders formatted ETA', () => {
    const segments = [
      { segmentIndex: 0, startByte: 0, endByte: 99, currentByte: 100, downloadedBytes: 100, isCompleted: true },
      { segmentIndex: 1, startByte: 100, endByte: 199, currentByte: 140, downloadedBytes: 40, isCompleted: false },
      { segmentIndex: 2, startByte: 200, endByte: 299, currentByte: 120, downloadedBytes: 20, isCompleted: false },
      { segmentIndex: 3, startByte: 300, endByte: 399, currentByte: 300, downloadedBytes: 0, isCompleted: false },
    ];

    render(
      <DownloadProgressModal
        isOpen={true}
        onClose={vi.fn()}
        onPause={vi.fn()}
        onResume={vi.fn()}
        onCancel={vi.fn()}
        task={{
          id: 't-eta',
          url: 'https://example.com/archive.7z',
          fileName: 'archive.7z',
          fileSize: 400,
          downloadedBytes: 160,
          status: 'downloading',
          categoryId: 'compressed',
          destinationPath: '~/Downloads/Compressed/archive.7z',
          supportsRange: true,
          numConnections: 4,
          speedLimitBytesPerSec: 0,
          speedBytesPerSec: 15_000_000,
          etaSeconds: 344,
          progressPercent: 40.0,
          createdAt: new Date().toISOString(),
          segments,
        }}
      />
    );

    expect(screen.getByText('5m 44s')).toBeInTheDocument();
    expect(screen.getByText('Parallel Connections (4 threads)')).toBeInTheDocument();
    expect(screen.getByText('1 / 4 completed')).toBeInTheDocument();
    expect(screen.getByTitle('Thread #1: 100.0%')).toBeInTheDocument();
    expect(screen.getByTitle('Thread #2: 40.0%')).toBeInTheDocument();
    expect(screen.getByTitle('Thread #3: 20.0%')).toBeInTheDocument();
    expect(screen.getByTitle('Thread #4: 0.0%')).toBeInTheDocument();
  });

  it('F09: Deleting failed/paused tasks cleans disk artifacts while deleting completed rows preserves finished files, and Cancel cancels properly', () => {
    const cancelSpy = vi.spyOn(nativeBridge, 'cancelRealDownload').mockResolvedValue(true);
    const now = new Date().toISOString();
    localStorage.setItem(
      'download_cepat_tasks',
      JSON.stringify([
        {
          id: 'task-failed',
          url: 'https://example.com/broken.7z',
          fileName: 'broken.7z',
          fileSize: 5_000_000,
          downloadedBytes: 500_000,
          status: 'error',
          errorMessage: 'Connection lost',
          categoryId: 'compressed',
          destinationPath: '~/Downloads/Compressed/broken.7z',
          supportsRange: true,
          numConnections: 8,
          speedLimitBytesPerSec: 0,
          speedBytesPerSec: 0,
          etaSeconds: 0,
          progressPercent: 0,
          createdAt: now,
        },
        {
          id: 'task-paused',
          url: 'https://example.com/partial.7z',
          fileName: 'partial.7z',
          fileSize: 10_000_000,
          downloadedBytes: 2_000_000,
          status: 'paused',
          categoryId: 'compressed',
          destinationPath: '~/Downloads/Compressed/partial.7z',
          supportsRange: true,
          numConnections: 8,
          speedLimitBytesPerSec: 0,
          speedBytesPerSec: 0,
          etaSeconds: 0,
          progressPercent: 20,
          createdAt: now,
        },
        {
          id: 'task-completed',
          url: 'https://example.com/done.7z',
          fileName: 'done.7z',
          fileSize: 4_000_000,
          downloadedBytes: 4_000_000,
          status: 'completed',
          categoryId: 'compressed',
          destinationPath: '~/Downloads/Compressed/done.7z',
          supportsRange: true,
          numConnections: 8,
          speedLimitBytesPerSec: 0,
          speedBytesPerSec: 0,
          etaSeconds: 0,
          progressPercent: 100,
          createdAt: now,
        },
      ])
    );

    try {
      render(<App />);

      // Mark failed task and completed task, then delete marked
      fireEvent.click(screen.getByLabelText('Mark broken.7z'));
      fireEvent.click(screen.getByLabelText('Mark done.7z'));
      fireEvent.click(screen.getByRole('button', { name: /Delete Marked \(2\)/i }));

      expect(cancelSpy).toHaveBeenCalledTimes(1);
      expect(cancelSpy).toHaveBeenCalledWith('task-failed', '~/Downloads/Compressed/broken.7z');
      expect(screen.queryByText('broken.7z')).not.toBeInTheDocument();
      expect(screen.queryByText('done.7z')).not.toBeInTheDocument();

      // Open details for paused task and click Cancel (should cancel + clean disk, not pause!)
      cancelSpy.mockClear();
      const partialRow = screen.getByText('partial.7z').closest('tr');
      expect(partialRow).not.toBeNull();
      fireEvent.doubleClick(partialRow!);
      fireEvent.click(screen.getByRole('button', { name: /^Cancel$/i }));

      expect(cancelSpy).toHaveBeenCalledTimes(1);
      expect(cancelSpy).toHaveBeenCalledWith('task-paused', '~/Downloads/Compressed/partial.7z');
      expect(screen.getByText('Canceled')).toBeInTheDocument();
    } finally {
      cancelSpy.mockRestore();
      localStorage.removeItem('download_cepat_tasks');
    }
  });

  it('F10: Batch wildcard expansion ([01-03], .ef2/.txt) and UI batch queue enqueue works', () => {
    expect(expandBatchPattern('https://cdn.test/vol_[01-03].zip')).toEqual([
      'https://cdn.test/vol_01.zip',
      'https://cdn.test/vol_02.zip',
      'https://cdn.test/vol_03.zip',
    ]);
    expect(expandBatchPattern('ftp://files.test/disk_[a-c].iso')).toEqual([
      'ftp://files.test/disk_a.iso',
      'ftp://files.test/disk_b.iso',
      'ftp://files.test/disk_c.iso',
    ]);
    expect(
      parseUrlListText('# comment\n<https://a.test/one.pdf>\nhttps://b.test/part_[1-2].7z')
    ).toEqual([
      'https://a.test/one.pdf',
      'https://b.test/part_1.7z',
      'https://b.test/part_2.7z',
    ]);

    try {
      render(<App />);
      fireEvent.click(screen.getByRole('button', { name: /Batch \/ Grab/i }));
      const textarea = screen.getByLabelText('URL List or Wildcard Pattern');
      fireEvent.change(textarea, {
        target: { value: 'https://mirror.example.com/archive_[01-02].zip' },
      });

      expect(screen.getByText('Expansion Preview (2 / 2 selected)')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: /Add to Queue/i }));

      expect(screen.getByText('archive_01.zip')).toBeInTheDocument();
      expect(screen.getByText('archive_02.zip')).toBeInTheDocument();
    } finally {
      localStorage.removeItem('download_cepat_tasks');
    }
  });

  it('F11: Context menu opens Properties with Refresh Download Address, live per-task speed cap, and MD5/SHA verification', async () => {
    const dlSpy = vi.spyOn(nativeBridge, 'triggerRealDownload').mockResolvedValue();
    const limitSpy = vi.spyOn(nativeBridge, 'updateDownloadSpeedLimit').mockResolvedValue(1);
    const hashSpy = vi.spyOn(nativeBridge, 'verifyFileChecksum').mockResolvedValue({
      md5: '900150983cd24fb0d6963f7d28e17f72',
      sha1: 'a9993e364706816aba3e25717850c26c9cd0d89d',
      sha256: 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad',
      sizeBytes: 3,
    });

    const now = new Date().toISOString();
    localStorage.setItem(
      'download_cepat_tasks',
      JSON.stringify([
        {
          id: 'task-expired',
          url: 'https://cdn-old.example.com/big.7z?token=expired',
          fileName: 'big.7z',
          fileSize: 1_000_000_000,
          downloadedBytes: 250_000_000,
          status: 'paused',
          categoryId: 'compressed',
          destinationPath: '~/Downloads/Compressed/big.7z',
          supportsRange: true,
          numConnections: 8,
          speedLimitBytesPerSec: 0,
          speedBytesPerSec: 0,
          etaSeconds: 0,
          progressPercent: 25,
          createdAt: now,
        },
      ])
    );

    try {
      render(<App />);
      const row = screen.getByText('big.7z').closest('tr')!;
      fireEvent.contextMenu(row, { clientX: 120, clientY: 120 });

      expect(screen.getByRole('menu', { name: 'Context Menu big.7z' })).toBeInTheDocument();
      fireEvent.click(screen.getByRole('menuitem', { name: /Verify Checksum/i }));

      fireEvent.click(screen.getByRole('button', { name: /Compute Checksum/i }));
      await waitFor(() => {
        expect(screen.getByText('900150983cd24fb0d6963f7d28e17f72')).toBeInTheDocument();
      });

      const hashInput = screen.getByLabelText('Reference Hash');
      fireEvent.change(hashInput, { target: { value: '900150983CD24FB0D6963F7D28E17F72' } });
      expect(screen.getByText(/MATCH — File integrity verified/i)).toBeInTheDocument();
      fireEvent.change(hashInput, { target: { value: '000000' } });
      expect(screen.getByText(/MISMATCH/i)).toBeInTheDocument();

      // Switch to Info & Refresh tab -> test Live Speed Limit + Refresh URL
      fireEvent.click(screen.getByRole('button', { name: /Info & Refresh Address/i }));
      fireEvent.change(screen.getByLabelText('Task Speed Limit KB/s'), {
        target: { value: '1500' },
      });
      fireEvent.click(screen.getByRole('button', { name: /Apply Live/i }));
      expect(limitSpy).toHaveBeenCalledWith(1500, 'task-expired');

      fireEvent.change(screen.getByLabelText('Fresh Refresh URL'), {
        target: { value: 'https://cdn-fresh.example.com/big.7z?token=valid2' },
      });
      fireEvent.click(screen.getByRole('button', { name: /Update Address & Resume/i }));

      expect(dlSpy).toHaveBeenCalledTimes(1);
      expect(dlSpy.mock.calls[0][1]).toBe('https://cdn-fresh.example.com/big.7z?token=valid2');
      expect(dlSpy.mock.calls[0][5]).toMatchObject({ allowUrlRefresh: true });
    } finally {
      dlSpy.mockRestore();
      limitSpy.mockRestore();
      hashSpy.mockRestore();
      localStorage.removeItem('download_cepat_tasks');
    }
  });

  it('F12: Scheduler time window evaluation, modal save, and queue priority reordering', () => {
    const overnight = {
      ...DEFAULT_SCHEDULER_CONFIG,
      enabled: true,
      startTime: '23:00',
      stopTime: '06:00',
      days: [1], // Monday
    };
    expect(isWithinScheduleWindow(overnight, new Date(2026, 10, 9, 23, 30))).toBe(true); // Mon 23:30
    expect(isWithinScheduleWindow(overnight, new Date(2026, 10, 9, 14, 0))).toBe(false); // Mon 14:00

    try {
      render(<App />);
      fireEvent.click(screen.getByRole('button', { name: /Schedule/i }));
      fireEvent.change(screen.getByLabelText('Action After Completion'), {
        target: { value: 'sleep' },
      });
      fireEvent.click(screen.getByRole('button', { name: /Save Schedule/i }));

      const saved = JSON.parse(localStorage.getItem('download_cepat_scheduler') || '{}');
      expect(saved.postAction).toBe('sleep');
    } finally {
      localStorage.removeItem('download_cepat_scheduler');
    }
  });

  it('F13: Proxy URL formatting, domain login lookup, and custom category rules work cleanly', () => {
    expect(
      formatProxyUrl({
        enabled: true,
        protocol: 'socks5',
        host: '127.0.0.1',
        port: '1080',
        username: 'u',
        password: 'p',
      })
    ).toBe('socks5://u:p@127.0.0.1:1080');

    const logins = [
      { id: '1', domain: 'files.example.org', username: 'alice', password: 'secret' },
    ];
    expect(findMatchingSiteLogin('ftp://sub.files.example.org/data.bin', logins)?.username).toBe(
      'alice'
    );
    expect(findMatchingSiteLogin('https://other.org/data.bin', logins)).toBeUndefined();

    const customRules = DEFAULT_CATEGORY_RULES.map((r) =>
      r.id === 'compressed' ? { ...r, saveDir: '~/Downloads/ISOs', extensions: ['iso', '7z'] } : r
    );
    expect(matchCategoryForFile('ubuntu.iso', customRules)).toEqual({
      categoryId: 'compressed',
      saveDir: '~/Downloads/ISOs',
    });
  });

  it('F14: Mini Drop Target and Clipboard Monitor extension filtering work correctly', () => {
    const clipCfg = { ...DEFAULT_CLIPBOARD_CONFIG, enabled: true };
    expect(shouldInterceptClipboardUrl('https://cdn.example.com/installer.dmg', clipCfg)).toBe(
      true
    );
    expect(shouldInterceptClipboardUrl('https://cdn.example.com/article.html', clipCfg)).toBe(
      false
    );

    try {
      render(<App />);
      fireEvent.click(screen.getByRole('button', { name: 'Toggle Drop Target' }));
      const dropZone = screen.getByTestId('mini-drop-target');
      expect(dropZone).toBeInTheDocument();

      fireEvent.drop(dropZone, {
        dataTransfer: {
          getData: (type: string) =>
            type === 'text/plain' ? 'https://cdn.test/one.zip\nhttps://cdn.test/two.zip' : '',
          files: [],
        },
      });
      expect(screen.getByText('Batch Downloader & Site Grabber')).toBeInTheDocument();
      expect(screen.getByText('Expansion Preview (2 / 2 selected)')).toBeInTheDocument();
    } finally {
      localStorage.removeItem('download_cepat_minidrop');
    }
  });

  it('F15: Defaults to EN and switches across 16 international locales including RTL Arabic', () => {
    try {
      render(<App />);

      const langSelect = screen.getByLabelText('Select Interface Language') as HTMLSelectElement;
      expect(langSelect.value).toBe('en');
      expect(langSelect.options.length).toBe(16);
      expect(screen.getByText('All Downloads')).toBeInTheDocument();
      expect(document.documentElement.lang).toBe('en');
      expect(document.documentElement.dir).toBe('ltr');

      // Switch to Indonesian
      fireEvent.change(langSelect, { target: { value: 'id' } });
      expect(screen.getByText('Semua Unduhan')).toBeInTheDocument();
      expect(screen.getByPlaceholderText('Cari file...')).toBeInTheDocument();
      expect(localStorage.getItem('download_cepat_lang')).toBe('id');
      expect(document.documentElement.lang).toBe('id');
      expect(document.documentElement.dir).toBe('ltr');

      // Switch to Bahasa Melayu
      fireEvent.change(screen.getByLabelText('Pilih Bahasa Antarmuka'), {
        target: { value: 'ms' },
      });
      expect(screen.getByText('Semua Muat Turun')).toBeInTheDocument();
      expect(localStorage.getItem('download_cepat_lang')).toBe('ms');

      // Switch to Japanese via Settings modal buttons
      fireEvent.click(screen.getByRole('button', { name: 'Tetapan & Had Kelajuan' }));
      fireEvent.click(screen.getByRole('button', { name: /日本語/i }));
      expect(screen.getByText('すべてのダウンロード')).toBeInTheDocument();
      expect(screen.getByPlaceholderText('ファイルを検索...')).toBeInTheDocument();
      expect(localStorage.getItem('download_cepat_lang')).toBe('ja');
      expect(document.documentElement.lang).toBe('ja');
      expect(document.documentElement.dir).toBe('ltr');

      // Switch to Arabic and verify RTL direction attribute
      fireEvent.click(screen.getByRole('button', { name: /العربية/i }));
      expect(document.documentElement.lang).toBe('ar');
      expect(document.documentElement.dir).toBe('rtl');
      expect(localStorage.getItem('download_cepat_lang')).toBe('ar');
    } finally {
      localStorage.removeItem('download_cepat_lang');
      document.documentElement.lang = 'en';
      document.documentElement.dir = 'ltr';
    }
  });

  it('F16: Displays official Ujang Mustamar Build Info and clean original attribution in UI', () => {
    render(<App />);

    // Open Build Info directly from Sidebar attribution footer
    const buildBtn = screen.getByRole('button', { name: /Ujang Mustamar/i });
    fireEvent.click(buildBtn);

    expect(screen.getByText('Official Build Information')).toBeInTheDocument();
    expect(screen.getAllByText('Ujang Mustamar').length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText('Copyright © 2026 Ujang Mustamar')).toBeInTheDocument();
    expect(screen.getByText('MIT')).toBeInTheDocument();
    expect(screen.getAllByText(/v1\.0\.5/).length).toBeGreaterThanOrEqual(2);

    // Switch to Browser Extension tab and confirm official module branding renders cleanly
    fireEvent.click(screen.getByRole('button', { name: 'Browser Extension' }));
    expect(screen.getByText('Browser Extension Module (Chrome & Firefox)')).toBeInTheDocument();
    expect(document.body.textContent).toContain('Download Cepat');
    expect(document.body.textContent).toContain('Ujang Mustamar');
  });

  it('F17: Preserves Windows/POSIX paths in path helpers and classifies .appimage under Programs', () => {
    expect(getParentDir('C:\\Users\\Alice\\Downloads\\file.zip')).toBe('C:\\Users\\Alice\\Downloads');
    expect(getParentDir('C:\\file.zip')).toBe('C:\\');
    expect(getParentDir('D:/Media/clip.mp4')).toBe('D:\\Media');
    expect(getParentDir('~/Downloads/Video/v.mp4')).toBe('~/Downloads/Video');
    expect(getParentDir('/home/alice/archive.tar.gz')).toBe('/home/alice');

    expect(joinDestinationPath('C:\\Users\\Alice\\Downloads\\', 'setup.exe')).toBe(
      'C:\\Users\\Alice\\Downloads\\setup.exe'
    );
    expect(joinDestinationPath('C:\\', 'root.zip')).toBe('C:\\root.zip');
    expect(joinDestinationPath('~/Downloads/Programs/', 'Tool.AppImage')).toBe(
      '~/Downloads/Programs/Tool.AppImage'
    );
    expect(joinDestinationPath('/', 'top.bin')).toBe('/top.bin');

    expect(matchCategoryForFile('Tool-x86_64.AppImage', DEFAULT_CATEGORY_RULES)).toEqual({
      categoryId: 'programs',
      saveDir: '~/Downloads/Programs',
    });

    render(<App />);
    fireEvent.click(screen.getByRole('button', { name: /Add URL/i }));
    const urlInput = screen.getByPlaceholderText(/https:\/\/example\.com\/file\.zip/i);
    fireEvent.change(urlInput, {
      target: { value: 'https://example.com/releases/App-1.0.0-x86_64.AppImage' },
    });

    const selects = screen.getAllByRole('combobox') as HTMLSelectElement[];
    expect(selects.some((s) => s.value === 'programs')).toBe(true);
    expect(screen.getByDisplayValue('~/Downloads/Programs')).toBeInTheDocument();
  });

  it('F18: Checks for app updates in Settings About tab, shows update banner, and invokes install', async () => {
    const checkSpy = vi.spyOn(nativeBridge, 'checkForAppUpdate').mockResolvedValue({
      currentVersion: '1.0.5',
      latestVersion: '1.0.6',
      updateAvailable: true,
      releaseUrl: 'https://github.com/moesstudio/download-cepat/releases/tag/v1.0.6',
      releaseNotes: 'Auto-update & bug fixes',
      publishedAt: '2026-11-17T00:00:00Z',
      assetName: 'Download.Cepat_1.0.6_x64-setup.exe',
      assetUrl: 'https://github.com/moesstudio/download-cepat/releases/download/v1.0.6/Download.Cepat_1.0.6_x64-setup.exe',
      assetSize: 4730815,
      assetSha256: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
      installKind: 'windows_nsis',
      inPlaceSupported: true,
    });
    const installSpy = vi.spyOn(nativeBridge, 'downloadAndInstallAppUpdate').mockResolvedValue({
      startedInPlace: false,
      message: 'Simulated v1.0.6 update verified by SHA-256.',
      assetPath: '/tmp/Download.Cepat_1.0.6_x64-setup.exe',
    });

    try {
      render(<App />);

      // Open About / Update card via sidebar attribution button
      fireEvent.click(screen.getByRole('button', { name: /Ujang Mustamar/i }));
      expect(screen.getByText('Application Updates (Auto-Update)')).toBeInTheDocument();

      // Toggle auto-update preference
      const autoCheckToggle = screen.getByLabelText('Automatically check for updates on launch') as HTMLInputElement;
      expect(autoCheckToggle.checked).toBe(true);
      fireEvent.click(autoCheckToggle);
      expect(localStorage.getItem('download_cepat_auto_update')).toBe('false');

      // Check for update and assert banner + install button
      fireEvent.click(screen.getByRole('button', { name: /Check for Updates/i }));
      await waitFor(() => {
        expect(checkSpy).toHaveBeenCalledTimes(1);
        expect(screen.getByText('Update v1.0.6 Available')).toBeInTheDocument();
        expect(screen.getByRole('region', { name: 'Application Updates (Auto-Update)' })).toBeInTheDocument();
      });

      const updateButtons = screen.getAllByRole('button', { name: /Update Now/i });
      expect(updateButtons.length).toBeGreaterThanOrEqual(1);
      fireEvent.click(updateButtons[0]);

      await waitFor(() => {
        expect(installSpy).toHaveBeenCalledTimes(1);
        expect(screen.getAllByText(/Simulated v1\.0\.6 update/).length).toBeGreaterThan(0);
      });
    } finally {
      checkSpy.mockRestore();
      installSpy.mockRestore();
      localStorage.removeItem('download_cepat_auto_update');
    }
  });

  it('F19: Syncs 32-thread setting to Rust runtime and renders 32 parallel segments in progress modal', () => {
    const syncSpy = vi.spyOn(nativeBridge, 'syncRuntimeSettings').mockResolvedValue();

    try {
      render(<App />);

      const settingsBtn = screen.getByTitle('Settings & Speed Limiter');
      fireEvent.click(settingsBtn);

      const selectInputs = screen.getAllByRole('combobox');
      const threadSelect = selectInputs.find((s) => s.innerHTML.includes('32 Threads'));
      const maxTasksSelect = selectInputs.find((s) => s.innerHTML.includes('5 Tasks'));
      expect(threadSelect).toBeDefined();
      expect(maxTasksSelect).toBeDefined();

      if (maxTasksSelect) {
        fireEvent.change(maxTasksSelect, { target: { value: '5' } });
      }
      if (threadSelect) {
        fireEvent.change(threadSelect, { target: { value: '32' } });
      }

      fireEvent.click(screen.getByRole('button', { name: /Save Settings/i }));

      expect(syncSpy).toHaveBeenLastCalledWith(
        expect.objectContaining({
          defaultConnections: 32,
          maxTasks: 5,
          videoSaveDir: '~/Downloads/Video',
          musicSaveDir: '~/Downloads/Music',
          torrentsSaveDir: '~/Downloads/Torrents',
        })
      );
      expect(screen.getByText('32 Threads / Task')).toBeInTheDocument();
      expect(screen.getByText('32 Threads')).toBeInTheDocument();

      const thirtyTwoSegments = Array.from({ length: 32 }, (_, idx) => ({
        segmentIndex: idx,
        startByte: idx * 1024,
        endByte: (idx + 1) * 1024 - 1,
        currentByte: idx === 0 ? 1024 : idx * 1024 + 512,
        downloadedBytes: idx === 0 ? 1024 : 512,
        isCompleted: idx === 0,
      }));

      render(
        <DownloadProgressModal
          isOpen={true}
          task={{
            id: 'task-32-video',
            url: 'https://www.youtube.com/watch?v=Jdj13fGMmmA',
            fileName: 'BigVideo_1080p.mp4',
            fileSize: 32768,
            downloadedBytes: 16896,
            status: 'downloading',
            categoryId: 'video',
            destinationPath: '~/Downloads/Video/BigVideo_1080p.mp4',
            supportsRange: true,
            numConnections: 32,
            speedLimitBytesPerSec: 0,
            speedBytesPerSec: 8_000_000,
            etaSeconds: 2,
            progressPercent: 51.5,
            segments: thirtyTwoSegments,
            createdAt: new Date().toISOString(),
          }}
          onClose={vi.fn()}
          onPause={vi.fn()}
          onResume={vi.fn()}
          onCancel={vi.fn()}
        />
      );

      expect(screen.getByText(/Parallel Connections \(32 threads\)/i)).toBeInTheDocument();
      expect(screen.getByText(/1 \/ 32 completed/i)).toBeInTheDocument();
    } finally {
      syncSpy.mockRestore();
      localStorage.removeItem('download_cepat_settings');
    }
  });

  it('F20: IDM-style non-scrolling wrapped toolbar exposes Torrent and Clear Done actions', () => {
    render(<App />);

    const toolbar = screen.getByTestId('main-toolbar');
    expect(toolbar.className).not.toContain('overflow-x-auto');

    // Torrent button opens AddUrlModal pre-configured for Torrents
    const torrentBtn = screen.getByRole('button', { name: 'Torrent' });
    fireEvent.click(torrentBtn);

    expect(screen.getByDisplayValue('~/Downloads/Torrents')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/i }));

    // Clear Done button clears all 5 completed seeded tasks cleanly and disables afterward
    const clearDoneBtn = screen.getByRole('button', { name: 'Clear Done' });
    expect(clearDoneBtn).not.toBeDisabled();
    fireEvent.click(clearDoneBtn);

    expect(screen.queryByText('download-cepat-main.zip')).not.toBeInTheDocument();
    expect(screen.getByText('No Downloads Yet')).toBeInTheDocument();
    expect(clearDoneBtn).toBeDisabled();
  });

  it('F21: BitTorrent magnet and .torrent classification, modal detection, and peer/seed badges work', async () => {
    const magnet =
      'magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Ubuntu+Desktop.iso';
    expect(isMagnetUri(magnet)).toBe(true);
    expect(isTorrentReference(magnet)).toBe(true);
    expect(isTorrentReference('https://mirror.example.org/ubuntu.torrent')).toBe(true);
    expect(matchCategoryForFile('ubuntu.torrent', DEFAULT_CATEGORY_RULES)).toEqual({
      categoryId: 'torrents',
      saveDir: '~/Downloads/Torrents',
    });
    expect(matchCategoryForFile('Ubuntu Desktop.iso', DEFAULT_CATEGORY_RULES, magnet)).toEqual({
      categoryId: 'torrents',
      saveDir: '~/Downloads/Torrents',
    });

    const clipCfg = { ...DEFAULT_CLIPBOARD_CONFIG, enabled: true };
    expect(shouldInterceptClipboardUrl(magnet, clipCfg)).toBe(true);
    expect(
      shouldInterceptClipboardUrl('https://mirror.example.org/releases/debian.torrent', clipCfg)
    ).toBe(true);

    render(<App />);
    fireEvent.click(screen.getByRole('button', { name: 'Torrent' }));

    const urlInput = screen.getByPlaceholderText(/magnet:/i);
    fireEvent.change(urlInput, { target: { value: magnet } });

    await screen.findByText('BitTorrent Detected');
    expect(
      await screen.findByText(/0123456789abcdef0123456789abcdef01234567/i)
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Start Download/i }));

    expect(screen.getAllByText('Ubuntu Desktop.iso').length).toBeGreaterThan(0);

    // Verify peer/seed pill renders cleanly for BitTorrent tasks in progress dialog
    render(
      <DownloadProgressModal
        isOpen={true}
        onClose={vi.fn()}
        onPause={vi.fn()}
        onResume={vi.fn()}
        onCancel={vi.fn()}
        task={{
          id: 'task-torrent-live',
          url: magnet,
          fileName: 'Ubuntu Desktop.iso',
          fileSize: 4_000_000_000,
          downloadedBytes: 1_000_000_000,
          status: 'downloading',
          categoryId: 'torrents',
          destinationPath: '~/Downloads/Torrents/Ubuntu Desktop.iso',
          supportsRange: true,
          numConnections: 16,
          speedLimitBytesPerSec: 0,
          speedBytesPerSec: 6_500_000,
          etaSeconds: 460,
          progressPercent: 25,
          peers: 14,
          seeds: 6,
          createdAt: new Date().toISOString(),
        }}
      />
    );
    expect(screen.getByText('14 peers • 6 seeds')).toBeInTheDocument();
  });

  it('F22: Standalone PopupDownloadWindow displays silent pre-download, supports Minimize/Later, and morphs in-place to mini_progress', async () => {
    const diskSpy = vi.spyOn(nativeBridge, 'checkDiskSpace').mockResolvedValue(50_000_000_000);
    const minSpy = vi.spyOn(nativeBridge, 'minimizePopupWindow').mockResolvedValue();
    const laterSpy = vi.spyOn(nativeBridge, 'queuePopupLater').mockResolvedValue();
    const promoteSpy = vi.spyOn(nativeBridge, 'promotePopupDownload').mockResolvedValue({
      popupId: 'dl-popup-1',
      mode: 'mini_progress',
      source: 'browser',
      initialMode: 'url',
      url: 'https://cdn.example.com/archive.zip',
      fileName: 'archive.zip',
      destinationPath: '~/Downloads/Compressed/archive.zip',
      categoryId: 'compressed',
      numConnections: 8,
      fileSize: 10_485_760,
      taskId: 'task-popup-100',
      prebuffer: {
        status: 'downloading',
        runId: 1,
        url: 'https://cdn.example.com/archive.zip',
        fileName: 'archive.zip',
        downloadedBytes: 5_242_880,
        totalBytes: 10_485_760,
        speedBytesPerSec: 2_097_152,
        progressPercent: 50,
        supportsRange: true,
      },
    });

    try {
      render(
        <I18nProvider lang="en" setLang={() => {}}>
          <PopupDownloadWindow
            initialSession={{
              popupId: 'dl-popup-1',
              mode: 'confirm',
              source: 'browser',
              initialMode: 'url',
              url: 'https://cdn.example.com/archive.zip',
              fileName: 'archive.zip',
              destinationPath: '~/Downloads/Compressed/archive.zip',
              categoryId: 'compressed',
              numConnections: 8,
              fileSize: 10_485_760,
              prebuffer: {
                status: 'downloading',
                runId: 1,
                url: 'https://cdn.example.com/archive.zip',
                fileName: 'archive.zip',
                downloadedBytes: 5_242_880,
                totalBytes: 10_485_760,
                speedBytesPerSec: 2_097_152,
                progressPercent: 50,
                supportsRange: true,
              },
            }}
          />
        </I18nProvider>
      );

      // Confirm mode displays silent pre-download progress
      expect(screen.getByText('Download File Info')).toBeInTheDocument();
      expect(screen.getByTestId('popup-prebuffer-banner')).toHaveTextContent(/50%/);
      expect(screen.getByTestId('popup-prebuffer-banner')).toHaveTextContent('2.00 MB/s');

      // Minimize button (header + footer) invokes native minimize
      const minimizeButtons = screen.getAllByRole('button', { name: /Minimize/i });
      expect(minimizeButtons.length).toBeGreaterThanOrEqual(1);
      fireEvent.click(minimizeButtons[0]);
      expect(minSpy).toHaveBeenCalledWith('dl-popup-1');

      // Download Later passes destination and queued metadata without discarding prebuffer
      fireEvent.click(screen.getByRole('button', { name: /Download Later/i }));
      await waitFor(() => {
        expect(laterSpy).toHaveBeenCalledWith(
          expect.objectContaining({
            popupId: 'dl-popup-1',
            url: 'https://cdn.example.com/archive.zip',
            fileName: 'archive.zip',
          })
        );
      });

      // Start Download calls promotePopupDownload and morphs same window into mini_progress
      fireEvent.click(screen.getByRole('button', { name: /Start Download/i }));
      await waitFor(() => {
        expect(promoteSpy).toHaveBeenCalledWith(
          expect.objectContaining({
            popupId: 'dl-popup-1',
            destinationPath: '~/Downloads/Compressed/archive.zip',
          })
        );
        expect(screen.getByRole('button', { name: /Main Window/i })).toBeInTheDocument();
        expect(screen.getByRole('button', { name: /^Pause$/i })).toBeInTheDocument();
      });
    } finally {
      diskSpy.mockRestore();
      minSpy.mockRestore();
      laterSpy.mockRestore();
      promoteSpy.mockRestore();
    }
  });
});
