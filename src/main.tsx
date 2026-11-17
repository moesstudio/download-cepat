import React, { Component, ErrorInfo, ReactNode, useEffect, useState } from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.tsx';
import { PopupDownloadWindow } from './components/PopupDownloadWindow.tsx';
import './index.css';
import { I18nProvider } from './i18n/index.tsx';
import { isRtlLanguage, isValidLanguage, LanguageCode, translate } from './i18n/translations.ts';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

class ErrorBoundary extends Component<Props, State> {
  public state: State = {
    hasError: false,
    error: null,
  };

  public static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  public componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Download Cepat runtime error caught by ErrorBoundary:', error, errorInfo);
  }

  public render() {
    if (this.state.hasError) {
      let savedLang: LanguageCode = 'en';
      try {
        const raw = localStorage.getItem('download_cepat_lang');
        if (isValidLanguage(raw)) savedLang = raw;
      } catch {}
      return (
        <div
          dir={isRtlLanguage(savedLang) ? 'rtl' : 'ltr'}
          style={{ padding: 24, background: '#0b1120', color: '#f8fafc', height: '100vh', fontFamily: 'sans-serif' }}
        >
          <h2 style={{ color: '#ef4444', fontSize: 18, fontWeight: 'bold' }}>
            {translate(savedLang, 'errorBoundary.title')}
          </h2>
          <p style={{ fontSize: 13, color: '#94a3b8', marginTop: 8 }}>
            {translate(savedLang, 'errorBoundary.desc')}
          </p>
          <pre style={{ background: '#1e293b', padding: 12, borderRadius: 6, fontSize: 12, overflow: 'auto', marginTop: 12 }}>
            {this.state.error?.stack || this.state.error?.message}
          </pre>
          <button
            onClick={() => {
              try { localStorage.clear(); } catch (e) {}
              window.location.reload();
            }}
            style={{ marginTop: 16, padding: '8px 16px', background: '#3b82f6', color: 'white', border: 'none', borderRadius: 6, cursor: 'pointer', fontWeight: 'bold' }}
          >
            {translate(savedLang, 'errorBoundary.resetBtn')}
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

const PopupWindowRoot: React.FC = () => {
  const [lang, setLang] = useState<LanguageCode>(() => {
    try {
      const saved = localStorage.getItem('download_cepat_lang');
      if (isValidLanguage(saved)) return saved;
    } catch {}
    return 'en';
  });

  useEffect(() => {
    if (typeof document !== 'undefined') {
      document.documentElement.lang = lang;
      document.documentElement.dir = isRtlLanguage(lang) ? 'rtl' : 'ltr';
    }
  }, [lang]);

  useEffect(() => {
    const onStorage = (e: StorageEvent) => {
      if (e.key === 'download_cepat_lang' && isValidLanguage(e.newValue)) {
        setLang(e.newValue);
      }
    };
    window.addEventListener('storage', onStorage);
    return () => window.removeEventListener('storage', onStorage);
  }, []);

  return (
    <I18nProvider lang={lang} setLang={setLang}>
      <PopupDownloadWindow />
    </I18nProvider>
  );
};

const isPopupWindow =
  typeof window !== 'undefined' &&
  new URLSearchParams(window.location.search).get('window') === 'popup';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      {isPopupWindow ? <PopupWindowRoot /> : <App />}
    </ErrorBoundary>
  </React.StrictMode>
);
