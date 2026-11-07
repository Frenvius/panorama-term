export interface PluginSource {
  dir: string;
  manifest: string | null;
}

export interface PluginScan {
  root: string;
  plugins: PluginSource[];
}

export type ThemeKind = 'dark' | 'light';

export interface PluginTheme {
  id: string;
  label: string;
  kind: ThemeKind;
  colors: Record<string, string>;
  terminal: { foreground?: string; cursor?: string; ansi?: string[] };
}

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
  contributes: { themes: PluginTheme[] };
}

export type Plugin = { dir: string; manifest: PluginManifest; enabled: boolean } | { dir: string; error: string };

export interface PluginsState {
  root: string | null;
  plugins: Plugin[];
  error: string | null;
}
