import React from 'react';
import { 
  Folder, 
  Archive, 
  FileText, 
  Music, 
  Cpu, 
  Film, 
  CheckCircle2, 
  Clock, 
  Layers,
  Info
} from 'lucide-react';
import { Category } from '../types.ts';
import { useI18n } from '../i18n/index.tsx';

interface CategorySidebarProps {
  selectedCategory: string;
  onSelectCategory: (categoryId: string) => void;
  categories: Category[];
  counts: Record<string, number>;
  defaultConnections?: number;
  version?: string;
  author?: string;
  onOpenBuildInfo?: () => void;
}

export const CategorySidebar: React.FC<CategorySidebarProps> = ({
  selectedCategory,
  onSelectCategory,
  categories,
  counts,
  defaultConnections = 8,
  version = '1.0.0',
  author = 'Ujang Mustamar',
  onOpenBuildInfo,
}) => {
  const { t } = useI18n();

  const getIcon = (id: string) => {
    switch (id) {
      case 'all': return <Layers className="w-4 h-4" />;
      case 'unfinished': return <Clock className="w-4 h-4 text-warning" />;
      case 'completed': return <CheckCircle2 className="w-4 h-4 text-success" />;
      case 'compressed': return <Archive className="w-4 h-4 text-amber-600" />;
      case 'documents': return <FileText className="w-4 h-4 text-blue-600" />;
      case 'music': return <Music className="w-4 h-4 text-purple-600" />;
      case 'programs': return <Cpu className="w-4 h-4 text-red-600" />;
      case 'video': return <Film className="w-4 h-4 text-emerald-600" />;
      default: return <Folder className="w-4 h-4 text-text-muted" />;
    }
  };

  const getCategoryLabel = (cat: Category) => {
    switch (cat.id) {
      case 'compressed': return t('sidebar.catCompressed');
      case 'documents': return t('sidebar.catDocuments');
      case 'music': return t('sidebar.catMusic');
      case 'programs': return t('sidebar.catPrograms');
      case 'video': return t('sidebar.catVideo');
      default: return cat.name;
    }
  };

  const systemCategories = [
    { id: 'all', name: t('sidebar.statusAll') },
    { id: 'unfinished', name: t('sidebar.statusUnfinished') },
    { id: 'completed', name: t('sidebar.statusFinished') },
  ];

  return (
    <aside className="w-60 bg-surface border-r border-border h-full flex flex-col p-3 space-y-6 select-none shrink-0">
      {/* Brand Header */}
      <div className="flex items-center space-x-2.5 px-2 py-1">
        <img src="/logo.png" alt="Download Cepat" className="w-8 h-8 rounded-lg shadow-sm object-cover border border-border/50" />
        <div>
          <h1 className="text-sm font-bold text-text tracking-tight">Download Cepat</h1>
          <p className="text-[11px] text-text-muted">{t('sidebar.subtitle')}</p>
        </div>
      </div>

      {/* Main Status Views */}
      <div className="space-y-1">
        <p className="px-2 text-[10px] font-semibold text-text-muted uppercase tracking-wider">{t('sidebar.sectionStatus')}</p>
        {systemCategories.map((cat) => {
          const isSelected = selectedCategory === cat.id;
          const count = counts[cat.id] || 0;
          return (
            <button
              key={cat.id}
              onClick={() => onSelectCategory(cat.id)}
              className={`w-full flex items-center justify-between px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors ${
                isSelected
                  ? 'bg-accent/10 text-accent font-semibold'
                  : 'text-text hover:bg-surface-hover'
              }`}
            >
              <div className="flex items-center space-x-2">
                {getIcon(cat.id)}
                <span>{cat.name}</span>
              </div>
              <span className={`text-[11px] font-mono px-1.5 py-0.2 rounded-full ${isSelected ? 'bg-accent text-on-accent' : 'bg-border/60 text-text-muted'}`}>
                {count}
              </span>
            </button>
          );
        })}
      </div>

      {/* Content Categories */}
      <div className="space-y-1 flex-1 overflow-y-auto">
        <p className="px-2 text-[10px] font-semibold text-text-muted uppercase tracking-wider">{t('sidebar.sectionCategories')}</p>
        {categories.map((cat) => {
          const isSelected = selectedCategory === cat.id;
          const count = counts[cat.id] || 0;
          return (
            <button
              key={cat.id}
              onClick={() => onSelectCategory(cat.id)}
              className={`w-full flex items-center justify-between px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors ${
                isSelected
                  ? 'bg-accent/10 text-accent font-semibold'
                  : 'text-text hover:bg-surface-hover'
              }`}
            >
              <div className="flex items-center space-x-2 truncate">
                {getIcon(cat.id)}
                <span className="truncate">{getCategoryLabel(cat)}</span>
              </div>
              <span className={`text-[11px] font-mono px-1.5 py-0.2 rounded-full ${isSelected ? 'bg-accent text-on-accent' : 'bg-border/60 text-text-muted'}`}>
                {count}
              </span>
            </button>
          );
        })}
      </div>

      {/* Footer Info */}
      <div className="p-2 bg-bg rounded-md border border-border text-[11px] text-text-muted space-y-1">
        <div className="flex justify-between">
          <span>{t('sidebar.engineLabel')}</span>
          <span className="font-semibold text-success font-mono">{t('sidebar.engineValue')}</span>
        </div>
        <div className="flex justify-between">
          <span>{t('sidebar.connectionsLabel')}</span>
          <span className="font-mono">{t('sidebar.connectionsValue', { count: defaultConnections })}</span>
        </div>
        <button
          type="button"
          onClick={onOpenBuildInfo}
          title={t('sidebar.buildInfoTitle', { author })}
          className="w-full pt-1 mt-1 border-t border-border/60 flex items-center justify-between text-[10px] text-text-muted hover:text-accent transition-colors"
        >
          <span className="truncate font-medium">{t('sidebar.buildInfoButton', { version, author })}</span>
          <Info className="w-3 h-3 shrink-0" />
        </button>
      </div>
    </aside>
  );
};
