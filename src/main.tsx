import React, { Component, ErrorInfo, ReactNode } from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.tsx';
import './index.css';
import { isValidLanguage, LanguageCode, translate } from './i18n/translations.ts';

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
      let savedLang: LanguageCode = 'id';
      try {
        const raw = localStorage.getItem('download_cepat_lang');
        if (isValidLanguage(raw)) savedLang = raw;
      } catch {}
      return (
        <div style={{ padding: 24, background: '#0b1120', color: '#f8fafc', height: '100vh', fontFamily: 'sans-serif' }}>
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

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
