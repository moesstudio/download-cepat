import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import App from '../App.tsx';
import { AddUrlModal } from '../components/AddUrlModal.tsx';
import { SegmentedProgressBar } from '../components/SegmentedProgressBar.tsx';
import { DownloadProgressModal } from '../components/DownloadProgressModal.tsx';
import * as nativeBridge from '../services/nativeBridge.ts';
import {
  expandBatchPattern,
  parseUrlListText,
  formatProxyUrl,
  findMatchingSiteLogin,
  matchCategoryForFile,
  isWithinScheduleWindow,
  shouldInterceptClipboardUrl,
  DEFAULT_CATEGORY_RULES,
  DEFAULT_CLIPBOARD_CONFIG,
  DEFAULT_SCHEDULER_CONFIG,
} from '../utils/downloadUtils.ts';

describe('Download Cepat UI Suite', () => {
  it('F01-H1: Renders main dashboard with default seeded tasks and category counts', () => {
    render(<App />);

    // Brand and headers
    expect(screen.getByText('Download Cepat')).toBeInTheDocument();
    expect(screen.getByText('High-Speed Engine v1.0')).toBeInTheDocument();

    // Default tasks rendered
    expect(screen.getByText('download-cepat-main.zip')).toBeInTheDocument();
    expect(screen.getByText('Alacritty-v0.17.0.dmg')).toBeInTheDocument();
    expect(screen.getByText('RFC_9110_HTTP_Semantics.pdf')).toBeInTheDocument();
    expect(screen.getByText('Yura Yunita - Dunia Tipu-Tipu.mp4')).toBeInTheDocument();
    expect(screen.getByText('Yura Yunita - Dunia Tipu-Tipu.mp3')).toBeInTheDocument();
  });

  it('F01-H2: Opens Add URL modal and adds a new download task', () => {
    render(<App />);

    // Click "Tambah URL"
    const addBtn = screen.getByRole('button', { name: /Tambah URL/i });
    fireEvent.click(addBtn);

    // Modal is opened
    expect(screen.getByText('Tambah Unduhan Baru')).toBeInTheDocument();

    // Fill URL
    const urlInput = screen.getByPlaceholderText(/https:\/\/example\.com\/file\.zip/i);
    fireEvent.change(urlInput, { target: { value: 'https://example.com/new_dataset.zip' } });

    // Click "Mulai Download"
    const submitBtn = screen.getByRole('button', { name: /Mulai Download/i });
    fireEvent.click(submitBtn);

    // Modal closed and new task is in table and active progress modal
    expect(screen.queryByText('Tambah Unduhan Baru')).not.toBeInTheDocument();
    expect(screen.getAllByText('new_dataset.zip').length).toBeGreaterThan(0);
  });

  it('F01-E1: Empty URL disables download button', () => {
    render(<App />);

    const addBtn = screen.getByRole('button', { name: /Tambah URL/i });
    fireEvent.click(addBtn);

    const submitBtn = screen.getByRole('button', { name: /Mulai Download/i });
    expect(submitBtn).toBeDisabled();
  });

  it('F02-H1: Filters task table when searching by filename', () => {
    render(<App />);

    const searchInput = screen.getByPlaceholderText('Cari file...');
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

    // Click "Jeda"
    const pauseBtn = screen.getByRole('button', { name: /Jeda/i });
    fireEvent.click(pauseBtn);

    // Status changes to Jeda
    expect(screen.getAllByText('Jeda').length).toBeGreaterThan(0);

    // Click "Lanjut"
    const resumeBtn = screen.getByRole('button', { name: /Lanjut/i });
    fireEvent.click(resumeBtn);

    expect(screen.getAllByText('Mengunduh').length).toBeGreaterThan(0);
  });

  it('F04-H1: Speed Limiter modal allows toggling and setting custom bandwidth limit', () => {
    render(<App />);

    const settingsBtn = screen.getByTitle('Pengaturan & Batas Kecepatan');
    fireEvent.click(settingsBtn);

    expect(screen.getByText('Pengaturan & Speed Limiter')).toBeInTheDocument();

    // Check speed limiter checkbox
    const checkbox = screen.getByLabelText(/Aktifkan Speed Limiter/i);
    fireEvent.click(checkbox);

    // Save
    const saveBtn = screen.getByRole('button', { name: /Simpan Pengaturan/i });
    fireEvent.click(saveBtn);

    expect(screen.queryByText('Pengaturan & Speed Limiter')).not.toBeInTheDocument();
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
    expect(screen.getByText(/Koneksi Paralel \(8 thread\)/i)).toBeInTheDocument();
  });

  it('F01-V1: Pasting video link detects video media and allows resolution and format selection', async () => {
    render(<App />);

    const addBtn = screen.getByRole('button', { name: /Tambah URL/i });
    fireEvent.click(addBtn);

    const urlInput = screen.getByPlaceholderText(/https:\/\/example.com\/file.zip/i);
    fireEvent.change(urlInput, { target: { value: 'https://www.youtube.com/watch?v=dQw4w9WgXcQ' } });

    // Video media detected card should appear after probe
    await screen.findByText('Media Video Terdeteksi');

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
    const submitBtn = screen.getByRole('button', { name: /Mulai Download/i });
    fireEvent.click(submitBtn);

    // Download started and appears in list
    expect(screen.getAllByText(/.*\.mp3/i).length).toBeGreaterThan(0);
  });

  it('F05: Multi-select checkboxes mark tasks and toolbar delete marked works', () => {
    render(<App />);

    // Mark the first file checkbox
    const markCheckbox = screen.getByLabelText(/Tandai download-cepat-main.zip/i);
    fireEvent.click(markCheckbox);

    // Delete marked button should appear in toolbar
    const deleteMarkedBtn = screen.getByRole('button', { name: /Hapus Ditandai \(1\)/i });
    expect(deleteMarkedBtn).toBeInTheDocument();

    // Click delete marked
    fireEvent.click(deleteMarkedBtn);

    // download-cepat-main.zip should no longer be in the document
    expect(screen.queryByText('download-cepat-main.zip')).not.toBeInTheDocument();
  });

  it('F06: Settings changes threads and max concurrent tasks', () => {
    render(<App />);

    const settingsBtn = screen.getByTitle('Pengaturan & Batas Kecepatan');
    fireEvent.click(settingsBtn);

    // Change thread count to 16
    const selectInputs = screen.getAllByRole('combobox');
    const threadSelect = selectInputs.find((s) => s.innerHTML.includes('16 Thread'));
    if (threadSelect) {
      fireEvent.change(threadSelect, { target: { value: '16' } });
    }

    // Save
    const saveBtn = screen.getByRole('button', { name: /Simpan Pengaturan/i });
    fireEvent.click(saveBtn);

    // Sidebar should now display 16 Thread
    expect(screen.getByText('16 Thread / Task')).toBeInTheDocument();
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

    expect(screen.getByText('Sesi Browser Aktif Terlampir')).toBeInTheDocument();
    expect(screen.getByDisplayValue('W11Pro.7z')).toBeInTheDocument();

    const submitBtn = screen.getByRole('button', { name: /Mulai Download/i });
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

    expect(screen.getByText('5m 44d')).toBeInTheDocument();
    expect(screen.getByText('Koneksi Paralel (4 thread)')).toBeInTheDocument();
    expect(screen.getByText('1 / 4 selesai')).toBeInTheDocument();
    expect(screen.getByTitle('Thread #1: 100.0%')).toBeInTheDocument();
    expect(screen.getByTitle('Thread #2: 40.0%')).toBeInTheDocument();
    expect(screen.getByTitle('Thread #3: 20.0%')).toBeInTheDocument();
    expect(screen.getByTitle('Thread #4: 0.0%')).toBeInTheDocument();
  });

  it('F09: Deleting failed/paused tasks cleans disk artifacts while deleting completed rows preserves finished files, and Batalkan cancels properly', () => {
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
          errorMessage: 'Koneksi terputus',
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
      fireEvent.click(screen.getByLabelText('Tandai broken.7z'));
      fireEvent.click(screen.getByLabelText('Tandai done.7z'));
      fireEvent.click(screen.getByRole('button', { name: /Hapus Ditandai \(2\)/i }));

      expect(cancelSpy).toHaveBeenCalledTimes(1);
      expect(cancelSpy).toHaveBeenCalledWith('task-failed', '~/Downloads/Compressed/broken.7z');
      expect(screen.queryByText('broken.7z')).not.toBeInTheDocument();
      expect(screen.queryByText('done.7z')).not.toBeInTheDocument();

      // Open details for paused task and click Batalkan (should cancel + clean disk, not pause!)
      cancelSpy.mockClear();
      const partialRow = screen.getByText('partial.7z').closest('tr');
      expect(partialRow).not.toBeNull();
      fireEvent.doubleClick(partialRow!);
      fireEvent.click(screen.getByRole('button', { name: /Batalkan/i }));

      expect(cancelSpy).toHaveBeenCalledTimes(1);
      expect(cancelSpy).toHaveBeenCalledWith('task-paused', '~/Downloads/Compressed/partial.7z');
      expect(screen.getByText('Dibatalkan')).toBeInTheDocument();
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
      const textarea = screen.getByLabelText('Daftar URL atau Pola Wildcard');
      fireEvent.change(textarea, {
        target: { value: 'https://mirror.example.com/archive_[01-02].zip' },
      });

      expect(screen.getByText('Pratinjau Ekspansi (2 / 2 dipilih)')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: /Tambahkan ke Antrian/i }));

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

      expect(screen.getByRole('menu', { name: 'Menu Konteks big.7z' })).toBeInTheDocument();
      fireEvent.click(screen.getByRole('menuitem', { name: /Verifikasi Checksum/i }));

      fireEvent.click(screen.getByRole('button', { name: /Hitung Checksum/i }));
      await waitFor(() => {
        expect(screen.getByText('900150983cd24fb0d6963f7d28e17f72')).toBeInTheDocument();
      });

      const hashInput = screen.getByLabelText('Hash Referensi');
      fireEvent.change(hashInput, { target: { value: '900150983CD24FB0D6963F7D28E17F72' } });
      expect(screen.getByText(/COCOK — Integritas file terverifikasi/i)).toBeInTheDocument();
      fireEvent.change(hashInput, { target: { value: '000000' } });
      expect(screen.getByText(/TIDAK COCOK/i)).toBeInTheDocument();

      // Switch to Info & Refresh tab -> test Live Speed Limit + Refresh URL
      fireEvent.click(screen.getByRole('button', { name: /Info & Refresh Alamat/i }));
      fireEvent.change(screen.getByLabelText('Limit Kecepatan Tugas KB/s'), {
        target: { value: '1500' },
      });
      fireEvent.click(screen.getByRole('button', { name: /Terapkan Live/i }));
      expect(limitSpy).toHaveBeenCalledWith(1500, 'task-expired');

      fireEvent.change(screen.getByLabelText('URL Baru Refresh'), {
        target: { value: 'https://cdn-fresh.example.com/big.7z?token=valid2' },
      });
      fireEvent.click(screen.getByRole('button', { name: /Perbarui Alamat & Lanjutkan/i }));

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
      fireEvent.click(screen.getByRole('button', { name: /Jadwal/i }));
      fireEvent.change(screen.getByLabelText('Aksi Setelah Selesai'), {
        target: { value: 'sleep' },
      });
      fireEvent.click(screen.getByRole('button', { name: /Simpan Jadwal/i }));

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
      expect(screen.getByText('Pratinjau Ekspansi (2 / 2 dipilih)')).toBeInTheDocument();
    } finally {
      localStorage.removeItem('download_cepat_minidrop');
    }
  });

  it('F15: Switches UI language across ID, EN, MS, JA and persists selection', () => {
    localStorage.removeItem('download_cepat_lang');
    try {
      render(<App />);

      const langSelect = screen.getByLabelText('Pilih Bahasa Antarmuka') as HTMLSelectElement;
      expect(langSelect.value).toBe('id');
      expect(screen.getByText('Semua Unduhan')).toBeInTheDocument();

      // Switch to English
      fireEvent.change(langSelect, { target: { value: 'en' } });
      expect(screen.getByText('All Downloads')).toBeInTheDocument();
      expect(screen.getByPlaceholderText('Search files...')).toBeInTheDocument();
      expect(localStorage.getItem('download_cepat_lang')).toBe('en');
      expect(document.documentElement.lang).toBe('en');

      // Switch to Bahasa Melayu
      fireEvent.change(screen.getByLabelText('Select Interface Language'), {
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
    } finally {
      localStorage.removeItem('download_cepat_lang');
      document.documentElement.lang = 'id';
    }
  });

  it('F16: Displays official Ujang Mustamar Build Info and clean original attribution in UI', () => {
    localStorage.removeItem('download_cepat_lang');
    render(<App />);

    // Open Build Info directly from Sidebar attribution footer
    const buildBtn = screen.getByRole('button', { name: /Ujang Mustamar/i });
    fireEvent.click(buildBtn);

    expect(screen.getByText('Informasi Build Resmi')).toBeInTheDocument();
    expect(screen.getAllByText('Ujang Mustamar').length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText('Copyright © 2026 Ujang Mustamar')).toBeInTheDocument();
    expect(screen.getByText('MIT')).toBeInTheDocument();
    expect(screen.getAllByText(/v1\.0\.0/).length).toBeGreaterThanOrEqual(2);

    // Switch to Browser Extension tab and confirm official module branding renders cleanly
    fireEvent.click(screen.getByRole('button', { name: 'Ekstensi Browser' }));
    expect(screen.getByText('Modul Ekstensi Browser (Chrome & Firefox)')).toBeInTheDocument();
    expect(document.body.textContent).toContain('Download Cepat');
    expect(document.body.textContent).toContain('Ujang Mustamar');
  });
});
